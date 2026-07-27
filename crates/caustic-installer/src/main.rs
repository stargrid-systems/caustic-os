mod app;
mod disk;
mod flash;
mod i18n;
mod usbboot;

pub fn main() -> iced::Result {
    iced::application(app::Installer::new, app::Installer::update, app::Installer::view)
        .title("Caustic Installer")
        .window_size((500.0, 400.0))
        .centered()
        .run()
}
