use std::io::Read;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    match args.get(1).map(String::as_str).unwrap_or("denied") {
        "denied" => {
            assert!(args.is_empty());
            assert!(std::env::vars().next().is_none());
            assert!(std::fs::read_dir("/").is_err());
            let mut input = String::new();
            assert_eq!(std::io::stdin().read_to_string(&mut input).unwrap(), 0);
            println!("UNEXPECTED ambient stdout");
            eprintln!("UNEXPECTED ambient stderr");
        }
        "granted" => {
            assert_eq!(args.len(), 4);
            let token = std::env::var("COMMAND_TOKEN").unwrap();
            assert_eq!(token, args[2]);
            assert_eq!(
                std::fs::read_to_string("/sandbox/input.txt").unwrap(),
                token
            );
            let written = std::fs::write("/sandbox/output.txt", &token);
            if args[3] == "read-only" {
                assert!(written.is_err());
            } else {
                written.unwrap();
            }
            assert!(std::fs::read_to_string("/sandbox/../outside.txt").is_err());
            println!("GUEST stdout {token}");
            eprintln!("GUEST stderr {token}");
        }
        "exit" => std::process::exit(args[2].parse().unwrap()),
        "trap" => panic!("command trap witness"),
        "spin" => loop {
            std::hint::spin_loop();
        },
        _ => panic!("unknown command mode"),
    }
}
