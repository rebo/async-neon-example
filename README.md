# Neon-async-example

An example app showing up to setup neon so that a background Rust task can get data aynschonously from Node.js.

The control flow is this..

Node.js invokes "perform_async_task" from index.rs

That task schedules two Neon Tasks a Dispatch task and MainBackgroundTask task.

The DispatchTask blocks until it receives a signal from MainBackgroundTask. (*)

When the MainBackgroundTask needs more data it unblocks the DispatchTask which then completes.

The completed Dispatch task gets new data from Node.js, sends this to the MainBackgroundTask, and then reschedules itself blocking again.

The main background task completes until it needs more data,(Step * above)

Notes: 

This makes use of the PersistentHandle pull request because you need to send 'boxed' jsHandles to your Dispatch and MainBackgroundTasks.

Hence you need to point to your own neon-bindings build, as demonstrated in Cargo.toml

See: https://github.com/neon-bindings/neon/pull/284






