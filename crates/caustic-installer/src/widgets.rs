use iced::widget::{Space, button, row, text};
use iced::{Center, Element, Fill};

pub fn pager<'a, Msg: Clone + 'a>(
    current_page: usize,
    total_pages: usize,
    on_prev: Option<Msg>,
    on_next: Option<Msg>,
    prev_label: &'a str,
    next_label: &'a str,
) -> Element<'a, Msg> {
    let mut prev = button(text(prev_label)).style(button::secondary);
    if let Some(msg) = on_prev.filter(|_| prev_in_bounds(current_page)) {
        prev = prev.on_press(msg);
    }

    let mut next = button(text(next_label)).style(button::secondary);
    if let Some(msg) = on_next.filter(|_| next_in_bounds(current_page, total_pages)) {
        next = next.on_press(msg);
    }

    let label = text(format!("{} / {}", current_page + 1, total_pages.max(1))).size(14);

    row![
        prev,
        Space::new().width(Fill),
        label,
        Space::new().width(Fill),
        next
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

const fn prev_in_bounds(current_page: usize) -> bool {
    current_page > 0
}

const fn next_in_bounds(current_page: usize, total_pages: usize) -> bool {
    current_page + 1 < total_pages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prev_in_bounds_at_boundaries() {
        assert!(!prev_in_bounds(0));
        assert!(prev_in_bounds(1));
        assert!(prev_in_bounds(5));
    }

    #[test]
    fn next_in_bounds_at_boundaries() {
        assert!(next_in_bounds(0, 3));
        assert!(next_in_bounds(1, 3));
        assert!(!next_in_bounds(2, 3));
        assert!(!next_in_bounds(3, 3));
        assert!(!next_in_bounds(0, 0));
    }

    #[test]
    fn prev_in_bounds_handles_single_page() {
        assert!(!prev_in_bounds(0));
    }
}
