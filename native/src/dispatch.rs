use std::sync::mpsc::Receiver;
use std::fmt;
use neon::js::JsBoolean;
use neon::mem::{Handle, PersistentHandle};
use std::error::Error;
use neon::task::Task;
use neon::scope::Scope;
use neon::vm::JsResult;
use neon::js::JsFunction;

// A neon Task that serves as an async interface to node.js
// the ::new constructor receives a mpsc receiver and boxed callback in order to trigger completion of
// the Task (and callback.) and then ressheduling the task
// the task is not rescheduled if the task recieves a  DispatchCommand::Cancel message.
pub struct DispatcherTask {
    // mpsc channel receiver needed to fetch data from Node.js runtime
    signal_receiver: Receiver<DispatchCommand>,

    // A Sender channel to send async data back from NodeJs to the MainBackgroundTask
    callback_handle: PersistentHandle,
}

impl DispatcherTask {
    pub fn new(rx: Receiver<DispatchCommand>, callback_handle: PersistentHandle) -> DispatcherTask {
        DispatcherTask {
            signal_receiver: rx,
            callback_handle: callback_handle,
        }
    }
}

pub enum DispatchCommand {
    Continue,
    Cancel,
}

#[derive(Debug)]
pub struct DispatchError {}

impl fmt::Display for DispatchError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Dispatch Error!")
    }
}

impl Error for DispatchError {
    fn description(&self) -> &str {
        "This is an error with the Dispatch Task"
    }

    fn cause(&self) -> Option<&Error> {
        None
    }
}

impl Task for DispatcherTask {
    type Output = DispatchCommand;
    type Error = DispatchError;

    // A JsBoolean to control whether the callback is fully executed.
    type JsEvent = JsBoolean;

    // This perform method should be running continually and enables communication with node.js
    fn perform(&mut self) -> Result<Self::Output, Self::Error> {
        // Don't do anything until signalled
        match self.signal_receiver.recv() {
            Ok(DispatchCommand::Continue) => Ok(DispatchCommand::Continue),
            Ok(DispatchCommand::Cancel) => Ok(DispatchCommand::Cancel),
            Err(_) => Err(DispatchError {}),
        }
    }

    // Either reschedule the command or not.
    fn complete<'a, T: Scope<'a>>(
        self,
        scope: &'a mut T,
        result: Result<Self::Output, Self::Error>,
    ) -> JsResult<Self::JsEvent> {
        match result {
            // If DispatchCommand::Coninue then reschedule the callback
            Ok(DispatchCommand::Continue) => {
                let callback: Handle<JsFunction> =
                    self.callback_handle.clone().into_handle(scope).check()?;
                self.schedule(callback);

                Ok(JsBoolean::new(scope, true))
            }

            // If DispatchCommand::Cancel then do not reschedule the callback
            Ok(DispatchCommand::Cancel) => {
                println!("DispatchTask has been Cancelled");
                Ok(JsBoolean::new(scope, false))
            }
            // Return false if you do not want the callback to fire
            // This currently swallows all errors as a JSBoolean false.
            Err(_) => Ok(JsBoolean::new(scope, false)),
        }
    }
}
