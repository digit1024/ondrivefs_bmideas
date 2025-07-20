use cosmic::prelude::*;
use cosmic::widget;
use cosmic::widget::{button, column, grid, icon, row, text};


#[derive(Debug, Clone)]
pub enum Message {
    
    FetchStatus,

    
    
}



pub struct Page {
    
    
}
impl Page {
    pub fn new() -> Self {
        Self {
            
        }
    }
    pub fn view(&self) -> cosmic::Element<Message> {
        widget::column().push(
            widget::text::body("Status")
        ).into()
    }
    
    pub fn update(&mut self, message: Message) -> cosmic::Task<cosmic::Action<Message>> {
        match message {
            Message::FetchStatus => {
                let set_window_title =  Task::none();
                cosmic::Task::batch(vec![ set_window_title])
            }
        }
    }
}
