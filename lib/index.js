var msg_buffer = {
    get_new_messages: function () {
        // Mock object and function to send data to rust background task
        return "Stub Node.js data";
    }
};

var addon = require('../native');

addon.perform_async_task(msg_buffer, (err, n) => {
    if (!err) {
        console.log("Async task finished back in node land now :)");
    }
});
