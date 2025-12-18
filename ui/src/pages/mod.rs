// SPDX-License-Identifier: MPL-2.0

pub mod about_element;
pub mod conflicts;
pub mod folders;
pub mod gallery;
pub mod logs;
pub mod queues;
pub mod status;

// Re-export for convenience
pub use conflicts::ConflictsPage;
pub use folders::Page as FoldersPage;
pub use gallery::Page as GalleryPage;
pub use logs::Page as LogsPage;
pub use queues::Page as QueuesPage;
pub use status::Page as StatusPage;
