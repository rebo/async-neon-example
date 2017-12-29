#[macro_use]
extern crate neon;

use neon::vm::{Call, JsResult};
use neon::js::{JsString, JsFunction, JsUndefined, JsObject, JsValue, JsBoolean, Object};
use neon::mem::{PersistentHandle, Handle};
use neon::scope::{Scope};
use neon::task::Task;

use std::sync::mpsc;
use std::sync::mpsc::{Sender, Receiver};

use std::time;

struct DispatcherTask{
    // mpsc channel receiver needed to fetch data from Node.js runtime
    signal_receiver : Receiver<bool>,

    // A Sender channel to send async data back from NodeJs to the MainBackgroundTask
    new_data_sender: Sender<String>,
    
    // boxed handle to the callback function scheduled on the dispatch task
    // Right now this is a stub blank handle as all work is done in the Dispatch::complete function
    // In theory this could be a boxed closure so that it could contain more context
    callback_handle: PersistentHandle,

    // the Node.js object that contains the function to retrieve Node.js data
    message_buffer_handle: PersistentHandle,
}

enum DispatchResult {
    Success,
    Failure,
}

impl Task for DispatcherTask {
    type Output = DispatchResult;
    type Error = DispatchResult;

    // The object passed to the callback function. in this case the object behind self.message_buffer_handle
    type JsEvent = JsObject;

    fn perform(&self) -> Result<Self::Output, Self::Error> {
        // Don't do anything until signalled
        match self.signal_receiver.recv() {
            Ok(_) => {
                Ok(DispatchResult::Success)
            },
            Err(_) => {
                Err(DispatchResult::Failure)
            }
        }   
    }
    
    fn complete<'a, T: Scope<'a>>(self, scope: &'a mut T, result: Result<Self::Output, Self::Error>) -> JsResult<Self::JsEvent> {
        match result {
            // Fire the callback if all is okay.  Dont really need the Ok Value
            Ok(_) =>{  
                // Should be ok to unwrap these due to Ok(_) branch..      
                let callback : Handle<JsFunction> = self.callback_handle.clone()
                    .into_handle(scope)
                    .check()
                    .unwrap();
                
                let msg_buffer: Handle  <JsObject> = self.message_buffer_handle.clone()
                    .into_handle(scope)
                    .check()
                    .unwrap();
                
                // invoke .get_new_messages on Node.js object "message_buffer"
                let get_new_messages = msg_buffer.get(scope, "get_new_messages")?.check::<JsFunction>()?;
                let args: Vec<Handle<JsValue>> = vec![];
                let new_messages : String = get_new_messages.call(scope, msg_buffer,args)?.check::<JsString>()?.value();
                
                // send the new messages to the MainBackgroundTask
                self.new_data_sender.send(new_messages).unwrap();
                
                // Reschedule this dispatch task
                self.schedule(callback);
                Ok(msg_buffer)
            },
            
            // Return a blank Js Object if we do not want the callback to fire.
            Err(_) => {
                Ok(JsObject::new(scope))
            }
        }
    }
}

struct MasterBackgroundTask{
    // Needs a mpsc channel Sender in order to message the dispatch task so it can trigger completion.
    dispatch_task_sender: Sender<bool>,
    new_data_receiver: Receiver<String>,
}

enum MasterBackgroundResult{
    Success,
}

impl Task for MasterBackgroundTask {

    // the return type of perform (within a result)
    type Output = MasterBackgroundResult;

    // the error type of perform (within a result)
    // this should never be needed
    type Error = MasterBackgroundResult;

    // the return type of the callback, counts the number of messages received from Node.js
    type JsEvent = JsBoolean;

    fn perform(&self) -> Result<Self::Output, Self::Error> {

        // loop for 20 seconds
        let starttime = time::Instant::now();
        let endtime = starttime + time::Duration::from_secs(5) + time::Duration::from_millis(2);
        let mut reset_time = time::Instant::now();

        while time::Instant::now() < endtime {
            
            if time::Instant::now() > reset_time + time::Duration::from_millis(100){
                reset_time = time::Instant::now();

                // trigger the dispatch to complete, then re-schedule itself
                self.dispatch_task_sender.send(true).unwrap();
                // Receive new data from the dispatch task (obtained from Node.js)
                let new_data_from_nodejs = self.new_data_receiver.recv().unwrap();
                
                
                let elapsed = starttime.elapsed();
                let seconds = elapsed.as_secs();
                let millisecs = elapsed.subsec_nanos()/ 1_000_000;
                println!("new data recieved asyncrously: {:?} at {:?} secs {:?} ms",new_data_from_nodejs, seconds, millisecs) ;
            }
        }
        
        Ok(MasterBackgroundResult::Success)
    }

    fn complete<'a, T: Scope<'a>>(self, scope: &'a mut T, _result: Result<Self::Output, Self::Error>) -> JsResult<Self::JsEvent> {
        println!("Main Background Task Complete");
        Ok(JsBoolean::new(scope,true))
    }
}

 pub fn perform_async_task(call: Call) -> JsResult<JsUndefined> {
    let scope = call.scope;

    // Node.js callback for MainBackgroundTask
    let main_task_callback = call.arguments.require(scope, 1)?.check::<JsFunction>()?;

    // Node.js object that is responsible for sending data async to the MainBackgroundTask
    let message_buffer = call.arguments.require(scope, 0)?.check::<JsObject>()?;

    // Channels for commmunication between MainBackgroundTask and DispatchTask
    let (dispatch_signal_sender, dispatch_signal_receiver): (Sender<bool>, Receiver<bool>) = mpsc::channel();
    let (new_data_sender, new_data_receiver): (Sender<String>, Receiver<String>) = mpsc::channel();
    
    // This callback fires when the dispatcher is completed
    // The callback is empty because all work is done in dispatcher complete Task
    // If boxed function closures worked then we would not need a different dispatcher
    // for each task. We would just change the callback closure and bake in context.
    // Unfortunately boxed function closures do not work therefore the callback is blank
    let callback = JsFunction::new(scope, stub_dispatch_callback )?;

    // persistent handles, js referenced objects need to be 'boxed' like this
    // with type erasure so that they can be sent accross threads
    let callback_handle = PersistentHandle::new(callback);
    let message_buffer_handle = PersistentHandle::new(message_buffer);
    
    // dispatch task object
    let dispatch = DispatcherTask{
        message_buffer_handle:  message_buffer_handle,
        signal_receiver: dispatch_signal_receiver,
        new_data_sender: new_data_sender,
        callback_handle: callback_handle,
        };

    dispatch.schedule(callback);

    (MasterBackgroundTask{
        dispatch_task_sender : dispatch_signal_sender,
        new_data_receiver : new_data_receiver, 
    }).schedule(main_task_callback);
    
    Ok(JsUndefined::new())
}

fn stub_dispatch_callback(_call: Call) -> JsResult<JsUndefined> {
        // does nothing
        Ok(JsUndefined::new())
}

register_module!(m, {
    m.export("perform_async_task", perform_async_task)
});
