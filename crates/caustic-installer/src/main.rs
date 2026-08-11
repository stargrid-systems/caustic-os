mod app;
mod disk;
mod flash;
mod i18n;
mod usbboot;
mod widgets;

#[cfg(target_os = "windows")]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--privileged-flash") {
        let code = flash::run_privileged_child(
            args.get(2).map_or("", String::as_str),
            args.get(3).map_or("", String::as_str),
            args.get(4).map_or("", String::as_str),
        );
        std::process::exit(code);
    }
    let simulate = args.iter().any(|a| a == "--simulate");
    let auto = args.iter().any(|a| a == "--auto");
    match run_gui(simulate, auto) {
        Ok(()) => std::process::exit(0),
        Err(_) => std::process::exit(1),
    }
}

#[cfg(not(target_os = "windows"))]
fn main() -> iced::Result {
    let simulate = std::env::args().any(|a| a == "--simulate");
    let auto = std::env::args().any(|a| a == "--auto");
    run_gui(simulate, auto)
}

fn run_gui(simulate: bool, auto: bool) -> iced::Result {
    iced::application(
        move || app::Installer::init(simulate, auto),
        app::Installer::update,
        app::Installer::view,
    )
    .title("Caustic OS Installer")
    .theme(|_state: &app::Installer| iced::Theme::Dark)
    .window_size((640.0, 680.0))
    .centered()
    .run()
}
