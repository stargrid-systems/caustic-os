use std::path::PathBuf;
use std::sync::Arc;

use iced::task::{sipper, Straw};
use iced::widget::{button, column, container, progress_bar, text};
use iced::{Center, Element, Fill, Task};

use caustic_installer_core::disk::{self, Disk};
use caustic_installer_core::{flash, oci};

const REGISTRY: &str = "ghcr.io/stargrid-systems/caustic-os";

pub struct Installer {
    step: Step,
    tags: Vec<String>,
    selected_tag: Option<usize>,
    error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    TagsLoaded(Result<Vec<String>, String>),
    TagSelected(usize),
    DownloadClicked,
    DownloadProgress(f32),
    DownloadFinished(Result<(), String>),
    DiskSelected(usize),
    FlashClicked,
    FlashProgress(f32),
    FlashFinished(Result<(), String>),
    Back,
}

enum Step {
    Loading,
    SelectRelease,
    Downloading { progress: f32, image_path: PathBuf },
    SelectDisk { image_path: PathBuf, disks: Vec<Disk>, selected: Option<usize> },
    Flashing { progress: f32 },
    Done,
}

impl Installer {
    pub fn new() -> (Self, Task<Message>) {
        let task = Task::perform(
            async { oci::list_tags(REGISTRY).await.map_err(|e| e.to_string()) },
            Message::TagsLoaded,
        );
        (
            Self {
                step: Step::Loading,
                tags: Vec::new(),
                selected_tag: None,
                error: None,
            },
            task,
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TagsLoaded(Ok(tags)) => {
                self.tags = tags;
                self.step = Step::SelectRelease;
                Task::none()
            }
            Message::TagsLoaded(Err(err)) => {
                self.error = Some(err);
                Task::none()
            }
            Message::TagSelected(index) => {
                self.selected_tag = Some(index);
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
                self.error = Some(err);
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
                self.error = Some(err);
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
            Step::Loading => column![text("Loading available releases...").size(20)].into(),
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
            column![content, text(format!("Error: {err}")).size(14)].spacing(10)
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

        for (i, tag) in self.tags.iter().enumerate() {
            col = col.push(
                button(text(tag.clone()))
                    .on_press(Message::TagSelected(i)),
            );
        }

        if self.selected_tag.is_some() {
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
            col = col.push(
                button(text(format!("{} ({} GB)", disk.name, disk.size_gb)))
                    .on_press(Message::DiskSelected(i)),
            );
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
        let Some(index) = self.selected_tag else {
            return Task::none();
        };
        let Some(tag) = self.tags.get(index).cloned() else {
            return Task::none();
        };

        let image_path = std::env::temp_dir()
            .join(format!("caustic-os-{tag}.img"))
            .to_string_lossy()
            .into_owned();

        let image_path_buf = PathBuf::from(&image_path);

        let straw = run_with_progress(move |progress| {
            oci::pull_image(
                REGISTRY.to_string(),
                tag,
                PathBuf::from(&image_path),
                progress,
            )
        });

        self.step = Step::Downloading { progress: 0.0, image_path: image_path_buf };
        Task::sip(straw, Message::DownloadProgress, Message::DownloadFinished)
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

        let image = image_path.clone();
        let target = disk.path.clone();

        let straw = run_with_progress(move |progress| {
            flash::flash_image(image, target, progress)
        });

        self.step = Step::Flashing { progress: 0.0 };
        Task::sip(straw, Message::FlashProgress, Message::FlashFinished)
    }
}

fn run_with_progress<F, Fut, E>(f: F) -> impl Straw<(), f32, String>
where
    F: FnOnce(Arc<dyn Fn(u64, u64) + Send + Sync>) -> Fut + Send,
    Fut: std::future::Future<Output = Result<(), E>> + Send,
    E: std::fmt::Display + Send + 'static,
{
    sipper(async move |mut straw| {
        let (tx, mut rx) = tokio::sync::watch::channel(0.0f32);

        let progress: Arc<dyn Fn(u64, u64) + Send + Sync> = Arc::new(move |done, total| {
            let pct = if total > 0 {
                done as f32 / total as f32 * 100.0
            } else {
                0.0
            };
            let _ = tx.send(pct);
        });

        let fut = f(progress);
        tokio::pin!(fut);

        loop {
            tokio::select! {
                result = &mut fut => {
                    return result.map_err(|e| e.to_string());
                }
                Ok(_) = rx.changed() => {
                    let pct = *rx.borrow();
                    let _ = straw.send(pct).await;
                }
            }
        }
    })
}
