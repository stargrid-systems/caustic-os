use iced::Task;
use iced::widget::{button, column, container, progress_bar, text};
use iced::{Center, Element, Fill};

use crate::disk::{self, Disk};
use crate::download;
use crate::flash;
use crate::github::{self, Release};

const REPO: &str = "stargrid-systems/caustic-os";

pub struct Installer {
    step: Step,
    releases: Vec<Release>,
    selected_release: Option<usize>,
    error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    ReleasesLoaded(Result<Vec<Release>, github::Error>),
    ReleaseSelected(usize),
    DownloadClicked,
    DownloadProgress(f32),
    DownloadFinished(Result<(), download::Error>),
    DiskSelected(usize),
    FlashClicked,
    FlashProgress(f32),
    FlashFinished(Result<(), flash::Error>),
    Back,
}

enum Step {
    Loading,
    SelectRelease,
    Downloading { progress: f32, image_path: String },
    SelectDisk { image_path: String, disks: Vec<Disk>, selected: Option<usize> },
    Flashing { progress: f32 },
    Done,
}

impl Installer {
    pub fn new() -> (Self, Task<Message>) {
        let task = Task::perform(github::fetch_releases(REPO), Message::ReleasesLoaded);
        (
            Self {
                step: Step::Loading,
                releases: Vec::new(),
                selected_release: None,
                error: None,
            },
            task,
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ReleasesLoaded(Ok(releases)) => {
                self.releases = releases;
                self.step = Step::SelectRelease;
                Task::none()
            }
            Message::ReleasesLoaded(Err(err)) => {
                self.error = Some(err.to_string());
                Task::none()
            }
            Message::ReleaseSelected(index) => {
                self.selected_release = Some(index);
                Task::none()
            }
            Message::DownloadClicked => self.start_download(),
            Message::DownloadProgress(progress) => {
                if let Step::Downloading { progress: p, .. } = &mut self.step {
                    *p = progress;
                }
                Task::none()
            }
            Message::DownloadFinished(Ok(())) => {
                if let Step::Downloading { image_path, .. } = &self.step {
                    let disks = disk::list_disks();
                    self.step = Step::SelectDisk {
                        image_path: image_path.clone(),
                        disks,
                        selected: None,
                    };
                }
                Task::none()
            }
            Message::DownloadFinished(Err(err)) => {
                self.error = Some(err.to_string());
                self.step = Step::SelectRelease;
                Task::none()
            }
            Message::DiskSelected(index) => {
                if let Step::SelectDisk { selected, .. } = &mut self.step {
                    *selected = Some(index);
                }
                Task::none()
            }
            Message::FlashClicked => self.start_flash(),
            Message::FlashProgress(progress) => {
                if let Step::Flashing { progress: p } = &mut self.step {
                    *p = progress;
                }
                Task::none()
            }
            Message::FlashFinished(Ok(())) => {
                self.step = Step::Done;
                Task::none()
            }
            Message::FlashFinished(Err(err)) => {
                self.error = Some(err.to_string());
                Task::none()
            }
            Message::Back => {
                self.error = None;
                self.step = Step::SelectRelease;
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let content: Element<'_, Message> = match &self.step {
            Step::Loading => column![text("Loading releases...").size(20)].into(),
            Step::SelectRelease => self.view_releases(),
            Step::Downloading { progress, .. } => {
                column![
                    text("Downloading image...").size(20),
                    progress_bar(0.0..=100.0, *progress),
                    text(format!("{progress:.1}%")).size(16),
                ]
                .into()
            }
            Step::SelectDisk { disks, selected, .. } => self.view_disks(disks, selected),
            Step::Flashing { progress } => {
                column![
                    text("Flashing image to disk...").size(20),
                    progress_bar(0.0..=100.0, *progress),
                    text(format!("{progress:.1}%")).size(16),
                ]
                .into()
            }
            Step::Done => {
                column![
                    text("Installation complete!").size(24),
                    text("You can now boot the device.").size(16),
                ]
                .into()
            }
        };

        let styled = if let Some(err) = &self.error {
            column![content, text(format!("Error: {err}")).size(14)]
                .spacing(10)
        } else {
            column![content]
        };

        container(
            styled
                .spacing(20)
                .width(Fill)
                .align_x(Center)
                .padding(40),
        )
        .center(Fill)
        .into()
    }

    fn view_releases(&self) -> Element<'_, Message> {
        let mut col = column![text("Select a release").size(24)];

        for (i, release) in self.releases.iter().enumerate() {
            let label = format!("{} ({})", release.tag, release.date);
            let btn = button(text(label));

            col = col.push(btn.on_press(Message::ReleaseSelected(i)));
        }

        if self.selected_release.is_some() {
            col = col.push(
                button(text("Download"))
                    .style(button::success)
                    .on_press(Message::DownloadClicked),
            );
        }

        col.spacing(10).into()
    }

    fn view_disks(&self, disks: &[Disk], selected: &Option<usize>) -> Element<'_, Message> {
        let mut col = column![text("Select target disk").size(24)];

        for (i, disk) in disks.iter().enumerate() {
            let label = format!("{} ({} GB)", disk.name, disk.size_gb);
            let btn = button(text(label));

            col = col.push(btn.on_press(Message::DiskSelected(i)));
        }

        if selected.is_some() {
            col = col.push(
                button(text("Flash image"))
                    .style(button::danger)
                    .on_press(Message::FlashClicked),
            );
        }

        col.spacing(10).into()
    }

    fn start_download(&mut self) -> Task<Message> {
        let Some(index) = self.selected_release else {
            return Task::none();
        };
        let Some(release) = self.releases.get(index).cloned() else {
            return Task::none();
        };

        let image_path = std::env::temp_dir()
            .join(format!("caustic-os-{}.img.xz", release.tag))
            .to_string_lossy()
            .into_owned();

        let (task, _handle) = Task::sip(
            download::download_image(
                release.image_url,
                image_path.clone(),
                release.image_checksum,
            ),
            Message::DownloadProgress,
            Message::DownloadFinished,
        )
        .abortable();

        self.step = Step::Downloading { progress: 0.0, image_path };
        task
    }

    fn start_flash(&mut self) -> Task<Message> {
        let Step::SelectDisk { image_path, disks, selected } = &self.step else {
            return Task::none();
        };

        let Some(index) = selected else {
            return Task::none();
        };

        let Some(disk) = disks.get(*index) else {
            return Task::none();
        };

        let path = image_path.clone();
        let target = disk.path.clone();

        let (task, _handle) = Task::sip(
            flash::flash_image(path, target),
            Message::FlashProgress,
            Message::FlashFinished,
        )
        .abortable();

        self.step = Step::Flashing { progress: 0.0 };
        task
    }
}
