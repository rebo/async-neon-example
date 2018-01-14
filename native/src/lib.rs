#[macro_use]
extern crate neon;
mod dispatch;

use neon::vm::{Call, JsResult};
use neon::js::{JsBoolean, JsFunction, JsObject, JsString, JsUndefined, JsValue, Object};
use neon::mem::{Handle, PersistentHandle};
use neon::scope::Scope;
use neon::task::Task;

use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};

use std::time;

use dispatch::{DispatchCommand, DispatcherTask};

struct MasterBackgroundTask {
    // Needs a mpsc channel Sender in order to message the dispatch task so it can trigger completion.
    dispatch_task_sender: Sender<DispatchCommand>,
    new_data_receiver: Receiver<String>,
}

enum MasterBackgroundResult {
    Success,
}

impl Task for MasterBackgroundTask {
    // the return type of perform (within a result)
    type Output = MasterBackgroundResult;

    // the error type of perform (within a result) this should never be needed
    type Error = MasterBackgroundResult;

    // the return type of the callback, counts the number of messages received from Node.js
    type JsEvent = JsBoolean;

    fn perform(&self) -> Result<Self::Output, Self::Error> {
        // loop for 20 seconds
        let starttime = time::Instant::now();
        let endtime = starttime + time::Duration::from_secs(2) + time::Duration::from_millis(2);
        let mut reset_time = time::Instant::now();

        while time::Instant::now() < endtime {
            if time::Instant::now() > reset_time + time::Duration::from_millis(100) {
                reset_time = time::Instant::now();

                // trigger the dispatch to complete, then re-schedule itself
                self.dispatch_task_sender
                    .send(DispatchCommand::Continue)
                    .expect("Error: I expect to be able to send to the dispatch task!");

                // Receive new data from the dispatch task (obtained from Node.js)
                let new_data_from_nodejs = self.new_data_receiver
                    .recv()
                    .expect("Error: I expect to be able to receive new data!");

                let elapsed = starttime.elapsed();
                let seconds = elapsed.as_secs();
                let millisecs = elapsed.subsec_nanos() / 1_000_000;

                println!(
                    "new data recieved asyncrously: {:?} at {:?} secs {:?} ms",
                    new_data_from_nodejs, seconds, millisecs
                );
            }
        }

        Ok(MasterBackgroundResult::Success)
    }

    fn complete<'a, T: Scope<'a>>(
        self,
        scope: &'a mut T,
        _result: Result<Self::Output, Self::Error>,
    ) -> JsResult<Self::JsEvent> {
        // send a cancel command, so the DispatchTask shuts down gracefully
        self.dispatch_task_sender
            .send(DispatchCommand::Cancel)
            .unwrap();

        println!("Main Background Task Complete");
        Ok(JsBoolean::new(scope, true))
    }
}

pub fn perform_async_task(call: Call) -> JsResult<JsUndefined> {
    let scope = call.scope;

    // Node.js callback for MainBackgroundTask
    let main_task_callback = call.arguments.require(scope, 1)?.check::<JsFunction>()?;

    // Node.js object that is responsible for sending data async to the MainBackgroundTask
    let message_buffer = call.arguments.require(scope, 0)?.check::<JsObject>()?;

    // We are storing it in a persistent handle so that it can be sent to the DispatchTask
    let message_buffer_handle = PersistentHandle::new(message_buffer);

    // Channels for commmunication between MainBackgroundTask and DispatchTask
    let (dispatch_signal_sender, dispatch_signal_receiver): (
        Sender<DispatchCommand>,
        Receiver<DispatchCommand>,
    ) = mpsc::channel();

    let (new_data_sender, new_data_receiver): (Sender<String>, Receiver<String>) = mpsc::channel();

    // This callback fires when the dispatcher is completed just returns a new JsObject
    let callback = JsFunction::new(
        scope,
        Box::new(move |inner| {
            // Callback function for the DispatcherTask

            // The object returned is irrelevant, it is not used by node.js

            // check to see if we should return early and not continue
            // this should be general to any DispatchTask callbbck
            // this is important because we dont want to send a message back to the main task
            // if the main task has completed.
            let cont = inner
                .arguments
                .require(inner.scope, 1)?
                .check::<JsBoolean>()?
                .value();

            if !cont {
                return Ok(JsUndefined::new());
            }

            // The below code is fired on on the main thread by the node.js runtime
            // It should be specific to the infomration data we want from the Nodejs runtime

            // btw we need to clone handles because otherwise Rust beleives they can be
            // referenced multiple times from a FnMut

            let msg_buffer: Handle<JsObject> = message_buffer_handle
                .clone()
                .into_handle(inner.scope)
                .check()?;

            // invoke .get_new_messages on Node.js object "message_buffer"
            let get_new_messages = msg_buffer
                .get(inner.scope, "get_new_messages")?
                .check::<JsFunction>()?;

            let args: Vec<Handle<JsValue>> = vec![];
            let new_messages: String = get_new_messages
                .call(inner.scope, msg_buffer, args)?
                .check::<JsString>()?
                .value();

            // send the new messages to the MainBackgroundTask
            new_data_sender
                .send(new_messages)
                .expect("Error: I expect to be able to send new data!");

            Ok(JsUndefined::new())
        }),
    )?;

    // Persistent handles, js referenced objects need to be 'boxed' like this
    // with type erasure so that they can be sent accross threads (stored in the DispatchTask)
    let callback_handle = PersistentHandle::new(callback);

    // Dispatch task object
    let dispatch = DispatcherTask::new(callback_handle, dispatch_signal_receiver);
    dispatch.schedule(callback);

    (MasterBackgroundTask {
        dispatch_task_sender: dispatch_signal_sender,
        new_data_receiver: new_data_receiver,
    }).schedule(main_task_callback);

    Ok(JsUndefined::new())
}

register_module!(m, { m.export("perform_async_task", perform_async_task) });
