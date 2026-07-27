mod app;
mod disk;
mod flash;
mod i18n;
mod usbboot;

pub fn main() -> iced::Result {
    iced::application(app::Installer::init, app::Installer::update, app::Installer::view)
        .title("Caustic Installer")
        .theme(|_state: &app::Installer| iced::Theme::Dark)
        .window_size((640.0, 560.0))
        .centered()
        .run()
}
