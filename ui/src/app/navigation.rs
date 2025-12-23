// SPDX-License-Identifier: MPL-2.0

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum PageId {
    #[default]
    Gallery,
    Status,
    Folders,
    Queues,
    Conflicts,
    Logs,
}




