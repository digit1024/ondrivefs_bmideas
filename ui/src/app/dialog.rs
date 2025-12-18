// SPDX-License-Identifier: MPL-2.0

use chrono::NaiveDate;
use cosmic::{
    iced::{
        alignment::{Horizontal, Vertical},
        Length,
    },
    widget::{self, calendar::CalendarModel},
};

use crate::app::actions::ApplicationAction;
use crate::app::Message;

/// Holds date information for calendar dialog
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateInfo {
    pub calendar: CalendarModel,
}

impl DateInfo {
    /// Create a new DateInfo with the given date
    pub fn new(date: NaiveDate) -> Self {
        Self {
            calendar: CalendarModel::new(date, date),
        }
    }
    
    /// Get the selected date from the calendar
    pub fn selected_date(&self) -> NaiveDate {
        self.calendar.selected
    }
}

#[derive(Debug, Clone)]
pub enum DialogAction {
    Open(DialogPage),
    Update(DialogPage),
    Close,
    Complete,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DialogPage {
    FullResetConfirm,
    StartDateCalendar(DateInfo),
    EndDateCalendar(DateInfo),
}

impl DialogPage {
    pub fn view(&self) -> widget::Dialog<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;

        match self {
            DialogPage::FullResetConfirm => widget::dialog()
                .title("Confirm Full Reset")
                .body("⚠️ WARNING: This will delete ALL data including:\n\n• All sync folders\n• All downloaded files\n• All database records\n• Authentication tokens\n\nThis action CANNOT be undone! The daemon will restart after reset.")
                .primary_action(widget::button::destructive("Confirm Reset").on_press(
                    Message::Application(ApplicationAction::Dialog(DialogAction::Complete)),
                ))
                .secondary_action(widget::button::standard("Cancel").on_press(
                    Message::Application(ApplicationAction::Dialog(DialogAction::Close)),
                )),
            DialogPage::StartDateCalendar(date_info) => {
                let date_info_clone_prev = date_info.clone();
                let date_info_clone_next = date_info.clone();
                widget::dialog()
                    .title("Select Start Date")
                    .primary_action(widget::button::suggested("OK").on_press_maybe(Some(
                        Message::Application(ApplicationAction::Dialog(DialogAction::Complete)),
                    )))
                    .secondary_action(widget::button::standard("Cancel").on_press(
                        Message::Application(ApplicationAction::Dialog(DialogAction::Close)),
                    ))
                    .control(
                        widget::column::with_children(vec![
                            widget::container(widget::calendar(
                                &date_info.calendar,
                                move |selected_date| {
                                    Message::Application(ApplicationAction::Dialog(
                                        DialogAction::Update(DialogPage::StartDateCalendar(DateInfo {
                                            calendar: CalendarModel::new(selected_date, selected_date),
                                        })),
                                    ))
                                },
                                move || {
                                    let mut new_info = date_info_clone_prev.clone();
                                    new_info.calendar.show_prev_month();
                                    Message::Application(ApplicationAction::Dialog(
                                        DialogAction::Update(DialogPage::StartDateCalendar(new_info))
                                    ))
                                },
                                move || {
                                    let mut new_info = date_info_clone_next.clone();
                                    new_info.calendar.show_next_month();
                                    Message::Application(ApplicationAction::Dialog(
                                        DialogAction::Update(DialogPage::StartDateCalendar(new_info))
                                    ))
                                },
                                chrono::Weekday::Mon,
                            ))
                            .width(Length::Fill)
                            .align_x(Horizontal::Center)
                            .align_y(Vertical::Center)
                            .into(),
                        ])
                        .spacing(spacing.space_s),
                    )
            }
            DialogPage::EndDateCalendar(date_info) => {
                let date_info_clone_prev = date_info.clone();
                let date_info_clone_next = date_info.clone();
                widget::dialog()
                    .title("Select End Date")
                    .primary_action(widget::button::suggested("OK").on_press_maybe(Some(
                        Message::Application(ApplicationAction::Dialog(DialogAction::Complete)),
                    )))
                    .secondary_action(widget::button::standard("Cancel").on_press(
                        Message::Application(ApplicationAction::Dialog(DialogAction::Close)),
                    ))
                    .control(
                        widget::column::with_children(vec![
                            widget::container(widget::calendar(
                                &date_info.calendar,
                                move |selected_date| {
                                    Message::Application(ApplicationAction::Dialog(
                                        DialogAction::Update(DialogPage::EndDateCalendar(DateInfo {
                                            calendar: CalendarModel::new(selected_date, selected_date),
                                        })),
                                    ))
                                },
                                move || {
                                    let mut new_info = date_info_clone_prev.clone();
                                    new_info.calendar.show_prev_month();
                                    Message::Application(ApplicationAction::Dialog(
                                        DialogAction::Update(DialogPage::EndDateCalendar(new_info))
                                    ))
                                },
                                move || {
                                    let mut new_info = date_info_clone_next.clone();
                                    new_info.calendar.show_next_month();
                                    Message::Application(ApplicationAction::Dialog(
                                        DialogAction::Update(DialogPage::EndDateCalendar(new_info))
                                    ))
                                },
                                chrono::Weekday::Mon,
                            ))
                            .width(Length::Fill)
                            .align_x(Horizontal::Center)
                            .align_y(Vertical::Center)
                            .into(),
                        ])
                        .spacing(spacing.space_s),
                    )
            }
        }
    }
}

