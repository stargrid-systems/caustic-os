mod app;
mod disk;
mod flash;
mod i18n;
mod usbboot;

pub fn main() -> iced::Result {
    let simulate = std::env::args().any(|a| a == "--simulate");

    iced::application(
        move || app::Installer::init(simulate),
        app::Installer::update,
        app::Installer::view,
    )
    .title("Caustic OS Installer")
    .theme(|_state: &app::Installer| iced::Theme::Dark)
    .window_size((640.0, 680.0))
    .centered()
    .run()
}
