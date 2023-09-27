use std::{io, thread};
use std::io::BufRead;
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, mpsc, Mutex};
use std::time::Duration;
use std::path::MAIN_SEPARATOR;
use tasklist;

fn main() {
    // Prompt the user to input the process name.
    println!("Enter the name of the process you want to search for:");
    let name = get_user_input();
    let process_name = format!("{}.exe", name.trim());
    let mut app = String::new();
    if let Some(process_location) = find_process_by_name(&process_name) {
        let location = process_location.as_str();

        if let Some(last_separator_index) = location.rfind(MAIN_SEPARATOR) {
            let result = &location[(last_separator_index + 1)..];
            app = result.to_string();
        }
    } else {
        println!("Process not found.");
        return;
    }

    // Prompt the user to input the restart interval.
    let restart_interval_hours: u64;
    println!("Enter the restart interval in hours:");
    loop {
        match get_user_input().parse::<u64>()
        {
            Ok(input) => {
                restart_interval_hours = input;
                break;
            }
            Err(_) => println!("Invalid input. Please enter a valid number."),
        };
    }

    // Create a signal to stop the monitoring thread.
    let exit_signal = Arc::new((Mutex::new(false), Condvar::new()));

    // Spawn a separate thread to run the monitoring and restarting logic.
    let exit_signal_clone = exit_signal.clone();
    let monitor_thread = thread::spawn(move || {
        loop {
            // Check the exit signal.
            let (lock, cvar) = &*exit_signal_clone;
            let should_exit = *lock.lock().unwrap();

            if should_exit {
                break;
            }

            if let Some(process_location) = find_process_by_name(&process_name) {
                // Sleep with the ability to be interrupted.
                let result = cvar
                    .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(restart_interval_hours * 60 * 60), |&mut should_exit| !should_exit)
                    .unwrap();
                unsafe
                    {
                        if result.1.timed_out() {
                            if let Err(err) = restart_process(&process_location, &process_name) {
                                eprintln!("Failed to restart process: {}", err);
                            }
                        }
                    }
            } else {
                eprintln!("Process {} not found. Exiting...", process_name);
                break;
            }
        }
        let (lock, cvar) = &*exit_signal_clone;
        *lock.lock().unwrap() = true;
        cvar.notify_all();
    });

    println!("Application: {}", app);
    println!("1. Exit");
    loop {
        let (lock, _cvar) = &*exit_signal;
        let monitoring_thread_exited = *lock.lock().unwrap();

        if monitoring_thread_exited {
            break;
        }
        let timeout_duration = Duration::from_millis(2000);
        let input = get_user_input_with_timeout(timeout_duration);
        match input
        {
            Some(string) =>
                {
                    match string.trim()
                    {
                        "1" => {
                            let (lock, cvar) = &*exit_signal;
                            *lock.lock().unwrap() = true;
                            cvar.notify_all();
                            println!("Exiting program.");
                            break;
                        }
                        _ => {
                            println!("Invalid option {}. Please select a valid option.", string.trim());
                            println!("1. Exit");
                        }
                    }
                }
            None => continue
        }
    }

    // Wait for the monitor_thread to finish.
    if let Err(err) = monitor_thread.join() {
        eprintln!("Error in the monitoring thread: {:?}", err);
    }
}

fn get_user_input() -> String {
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read input");
    input.trim().to_string()
}

fn get_user_input_with_timeout(timeout: Duration) -> Option<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut input = String::new();
        let stdin = io::stdin();
        let mut handle = stdin.lock();

        if handle.read_line(&mut input).is_ok() {
            tx.send(input.trim().to_string()).ok();
        }
    })
        .join().expect("Error in input thread joining");
    let result = rx.recv_timeout(timeout);
    match result
    {
        Err(_) => None,
        Ok(input) => Some(input)
    }
}

fn find_process_by_name(process_name: &str) -> Option<String> {
    unsafe
        {
            let list = tasklist::Tasklist::new();
            for process in list {
                if process.pname.as_str().to_lowercase() == process_name.to_lowercase() {
                    return Some(process.get_path().to_string());
                }
            }
            None
        }
}

unsafe fn restart_process(executable_path: &str, process_name: &str) -> Result<(), String> {
    let pid = find_pid(process_name);

    if pid == 0 {
        let message = format!("Program {} isn't running.",process_name);
        return Err(message);
    }

    let kill_command = format!("taskkill /F /PID {}", pid);

    match Command::new("cmd")
        .arg("/C")
        .arg(&kill_command)
        .stdout(Stdio::null())
        .status()
    {
        Ok(status) => {
            if !status.success() {
                eprintln!("Error terminating process with PID: {}", pid);
            }
        }
        Err(err) => {
            eprintln!("Error executing command: {}", err);
        }
    }

    match Command::new(executable_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn() {
        Ok(_) => Ok(()),
        Err(err) => Err(format!("Failed to restart process: {}", err)),
    }
}

unsafe fn find_pid(process_name: &str) -> u32 {
    let list = tasklist::Tasklist::new();
    for process in list {
        if process.pname.as_str().to_lowercase() == process_name.to_lowercase() {
            return process.pid;
        }
    }
    0
}
