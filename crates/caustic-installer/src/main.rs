mod app;
mod disk;
mod flash;
mod i18n;
mod usbboot;
mod widgets;

fn main() -> iced::Result {
    let simulate = std::env::args().any(|a| a == "--simulate");
    let auto = std::env::args().any(|a| a == "--auto");

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
