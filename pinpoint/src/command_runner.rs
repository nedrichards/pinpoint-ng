use gtk::gio;
use std::cell::{Cell, RefCell};
use std::ffi::OsStr;
use std::rc::Rc;

#[derive(Clone, Default)]
pub struct CommandRunner(Rc<Inner>);

#[derive(Default)]
struct Inner {
    process: RefCell<Option<gio::Subprocess>>,
    process_group: Cell<Option<i32>>,
    status: RefCell<Option<Result<(), String>>>,
    generation: Cell<u64>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(process) = self.process.get_mut().take() {
            terminate_process_group(&process, self.process_group.get_mut().take());
        }
    }
}

fn terminate_process_group(process: &gio::Subprocess, process_group: Option<i32>) {
    if let Some(process_group) = process_group {
        // SAFETY: `process_group` is parsed from the PID returned for the child
        // after its child-setup callback created a fresh session. A negative PID
        // addresses only that process group; SIGKILL requires no shared memory.
        unsafe {
            libc::kill(-process_group, libc::SIGKILL);
        }
    }
    process.force_exit();
}

impl CommandRunner {
    pub fn run(&self, command: &str) -> Result<(), String> {
        self.run_with_callback(command, |_| {})
    }

    pub fn run_with_callback(
        &self,
        command: &str,
        finished: impl FnOnce(Result<(), String>) + 'static,
    ) -> Result<(), String> {
        if self.0.process.borrow().is_some() {
            return Err("a slide command is already running".into());
        }
        let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::NONE);
        launcher.set_child_setup(|| {
            // SAFETY: `setsid` is async-signal-safe and called in the launcher
            // child before exec, with no Rust state shared back to the parent.
            unsafe {
                libc::setsid();
            }
        });
        let process = launcher
            .spawn(&[OsStr::new("/bin/sh"), OsStr::new("-c"), OsStr::new(command)])
            .map_err(|error| error.to_string())?;
        let generation = self.0.generation.get().wrapping_add(1);
        self.0.generation.set(generation);
        *self.0.status.borrow_mut() = None;
        self.0.process_group.set(
            process
                .identifier()
                .and_then(|identifier| identifier.parse::<i32>().ok()),
        );
        *self.0.process.borrow_mut() = Some(process.clone());
        let weak = Rc::downgrade(&self.0);
        process.wait_check_async(None::<&gio::Cancellable>, move |result| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if inner.generation.get() != generation {
                return;
            }
            let completed = inner.process.borrow_mut().take();
            if let Some(completed) = completed.as_ref() {
                terminate_process_group(completed, inner.process_group.take());
            }
            let result = result.map_err(|error| error.to_string());
            if let Err(error) = &result {
                eprintln!("pinpoint: slide command failed: {error}");
            } else {
                eprintln!("PINPOINT COMMAND finished status=success");
            }
            *inner.status.borrow_mut() = Some(result.clone());
            finished(result);
        });
        eprintln!("PINPOINT COMMAND started sandboxed=true");
        Ok(())
    }

    pub fn status(&self) -> Option<Result<(), String>> {
        self.0.status.borrow().clone()
    }

    pub fn is_running(&self) -> bool {
        self.0.process.borrow().is_some()
    }

    pub fn cancel(&self, reason: &str) {
        self.0
            .generation
            .set(self.0.generation.get().wrapping_add(1));
        if let Some(process) = self.0.process.borrow_mut().take() {
            terminate_process_group(&process, self.0.process_group.take());
            eprintln!("PINPOINT COMMAND cancelled reason={reason}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn completed_shell_cannot_leave_a_background_child_running() {
        let pid_file =
            std::env::temp_dir().join(format!("pinpoint-command-child-{}.pid", std::process::id()));
        let runner = CommandRunner::default();
        runner
            .run(&format!("sleep 30 & echo $! > {}", pid_file.display()))
            .expect("launch command");
        let context = gtk::glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_secs(2);
        while runner.status().is_none() && Instant::now() < deadline {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(runner.status(), Some(Ok(())));
        let pid = std::fs::read_to_string(&pid_file)
            .expect("background child pid")
            .trim()
            .parse::<i32>()
            .expect("numeric child pid");
        let reap_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            // SAFETY: signal 0 only tests whether the recorded synthetic child
            // PID still exists; it does not deliver a signal.
            let alive = unsafe { libc::kill(pid, 0) == 0 };
            if !alive || Instant::now() >= reap_deadline {
                assert!(!alive, "background child survived its command group");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = std::fs::remove_file(pid_file);
    }
}
