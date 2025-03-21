use sysinfo::System;

#[tokio::main]
async fn main() {
    let mut system = System::new();
    system.refresh_processes();

    let process_name = "qdrant.ext";

    let processes = system.processes_by_name(process_name);

    processes.into_iter().for_each(|process| {
        process.kill_with(sysinfo::Signal::Kill);
    });
}
