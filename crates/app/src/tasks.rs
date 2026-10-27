//! Work that runs off the interface thread. A task is a closure run on its own thread; when it ends, its result is handed
//! to a second closure that runs on the interface thread with the whole app in hand.

use std::sync::mpsc::{Receiver, Sender, channel};

type Finish = Box<dyn FnOnce(&mut crate::App, &egui::Context) + Send>;

/// What the app keeps of its running tasks.
pub struct Tasks {
    sender: Sender<(u64, Finish)>,
    receiver: Receiver<(u64, Finish)>,
    next: u64,
    running: Vec<(u64, String)>,
}

impl Default for Tasks {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Tasks { sender, receiver, next: 1, running: Vec::new() }
    }
}

impl Tasks {
    /// The names of the tasks running now (for a progress line).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.running.iter().map(|(_, n)| n.as_str())
    }

    pub fn is_running(&self, name: &str) -> bool {
        self.running.iter().any(|(_, n)| n == name)
    }
}

/// Runs `work` on a thread named `name`; its result goes to `done` on the interface thread. Fails only if the thread cannot
/// start.
pub fn spawn<T: Send + 'static>(
    app: &mut crate::App,
    name: &str,
    _kind: Option<&str>,
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(&mut crate::App, &egui::Context, T) + Send + 'static,
) -> Result<(), String> {
    let id = app.tasks.next;
    app.tasks.next += 1;
    let sender = app.tasks.sender.clone();
    let ctx = app.ctx.clone();
    std::thread::Builder::new()
        .name(format!("task-{name}"))
        .spawn(move || {
            // a task that panics ends without a result: the app goes on
            let Ok(result) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) else {
                let _ = sender.send((id, Box::new(|_, _| {})));
                ctx.request_repaint();
                return;
            };
            let _ = sender.send((id, Box::new(move |app, ctx| done(app, ctx, result))));
            ctx.request_repaint();
        })
        .map_err(|e| format!("could not start {name}: {e}"))?;
    app.tasks.running.push((id, name.to_string()));
    Ok(())
}

/// Runs the finished tasks' second halves. Called once a frame.
pub fn poll(app: &mut crate::App, ctx: &egui::Context) {
    let mut finished = Vec::new();
    while let Ok(item) = app.tasks.receiver.try_recv() {
        finished.push(item);
    }
    for (id, finish) in finished {
        app.tasks.running.retain(|(i, _)| *i != id);
        finish(app, ctx);
    }
}
