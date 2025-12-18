// SPDX-License-Identifier: MPL-2.0

#![allow(dead_code)]

#[derive(Debug, Clone)]
pub enum Message {
    FetchLogs,
    LogsLoaded(Result<Vec<String>, String>),
    Refresh,
    AutoRefresh,
    TogglePause,
}

