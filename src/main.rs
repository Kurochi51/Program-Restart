use std::{io, thread};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use tasklist;

fn main() {
    // Prompt the user to input the process name.
    println!("Enter the name of the process you want to search for:");
    let name = get_user_input();
    let process_name = format!("{}.exe", name.trim());
    let app = process_name.clone();

    // Prompt the user to input the restart interval.
    println!("Enter the restart interval in hours:");
    let restart_interval_hours = match get_user_input().parse::<u64>() {
        Ok(num) => num,
        Err(_) => {
            println!("Invalid input. Please enter a valid number.");
            return;
        }
    };

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
                eprintln!("Process {} not found. Exiting.", process_name);
                break;
            }
        }
    });

    println!("Application: {}", app);
    println!("1. Exit");
    // Wait for the user's input.
    loop {
        let user_choice = get_user_input();
        match user_choice.trim().as_ref() {
            "1" => {
                // Set the exit signal to stop the monitor_thread.
                let (lock, cvar) = &*exit_signal;
                *lock.lock().unwrap() = true;
                cvar.notify_all();
                break;
            }
            _ => {
                println!("Invalid option. Please select a valid option.");
            }
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
        return Err("Error in finding the Process ID".to_string());
    }

    let kill_command = format!("taskkill /F /PID {}", pid);

    match Command::new("cmd")
        .arg("/C")
        .arg(&kill_command)
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
