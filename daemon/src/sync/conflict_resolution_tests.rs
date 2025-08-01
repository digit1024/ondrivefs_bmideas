#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::persistency::processing_item_repository::{ProcessingItem, ChangeType, ChangeOperation};
    use crate::onedrive_service::onedrive_models::{DriveItem, File, Folder};
    use chrono::Utc;

    fn create_test_drive_item(name: &str, size: i64, last_modified: &str) -> DriveItem {
        DriveItem {
            id: format!("test-{}", name),
            name: Some(name.to_string()),
            size: Some(size),
            etag: Some("test-etag".to_string()),
            created_date: Some(Utc::now().to_rfc3339()),
            last_modified: Some(last_modified.to_string()),
            file: Some(File {
                mime_type: Some("text/plain".to_string()),
                hashes: None,
            }),
            folder: None,
            parent_reference: None,
            download_url: None,
            deleted: None,
        }
    }

    fn create_test_processing_item(
        drive_item: DriveItem,
        change_type: ChangeType,
        change_operation: ChangeOperation,
    ) -> ProcessingItem {
        ProcessingItem {
            id: Some(1),
            drive_item,
            status: crate::persistency::processing_item_repository::ProcessingStatus::New,
            error_message: None,
            last_status_update: None,
            retry_count: 0,
            priority: 0,
            change_type,
            change_operation,
            conflict_resolution: None,
            validation_errors: Vec::new(),
            user_decision: None,
        }
    }

    #[test]
    fn test_always_remote_strategy() {
        let strategy = strategies::AlwaysRemoteStrategy;
        let item = create_test_processing_item(
            create_test_drive_item("test.txt", 1000, "2024-01-01T00:00:00Z"),
            ChangeType::Local,
            ChangeOperation::Update,
        );

        let resolution = strategy.resolve_conflict(&item);
        assert_eq!(resolution, ConflictResolution::UseRemote);
    }

    #[test]
    fn test_always_local_strategy() {
        let strategy = strategies::AlwaysLocalStrategy;
        let item = create_test_processing_item(
            create_test_drive_item("test.txt", 1000, "2024-01-01T00:00:00Z"),
            ChangeType::Remote,
            ChangeOperation::Update,
        );

        let resolution = strategy.resolve_conflict(&item);
        assert_eq!(resolution, ConflictResolution::UseLocal);
    }

    #[test]
    fn test_manual_strategy() {
        let strategy = strategies::ManualStrategy;
        let item = create_test_processing_item(
            create_test_drive_item("test.txt", 1000, "2024-01-01T00:00:00Z"),
            ChangeType::Local,
            ChangeOperation::Update,
        );

        let resolution = strategy.resolve_conflict(&item);
        assert_eq!(resolution, ConflictResolution::Manual);
    }

    #[test]
    fn test_conflict_resolution_factory() {
        use onedrive_sync_lib::config::ConflictResolutionStrategy;

        let item = create_test_processing_item(
            create_test_drive_item("test.txt", 1000, "2024-01-01T00:00:00Z"),
            ChangeType::Local,
            ChangeOperation::Update,
        );

        // Test AlwaysRemote
        let strategy = ConflictResolutionFactory::create_strategy(&ConflictResolutionStrategy::AlwaysRemote);
        assert_eq!(strategy.resolve_conflict(&item), ConflictResolution::UseRemote);

        // Test AlwaysLocal
        let strategy = ConflictResolutionFactory::create_strategy(&ConflictResolutionStrategy::AlwaysLocal);
        assert_eq!(strategy.resolve_conflict(&item), ConflictResolution::UseLocal);

        // Test Manual
        let strategy = ConflictResolutionFactory::create_strategy(&ConflictResolutionStrategy::Manual);
        assert_eq!(strategy.resolve_conflict(&item), ConflictResolution::Manual);
    }

    #[test]
    fn test_conflict_resolution_serialization() {
        // Test that ConflictResolution can be serialized/deserialized
        let resolutions = vec![
            ConflictResolution::UseRemote,
            ConflictResolution::UseLocal,
            ConflictResolution::Skip,
            ConflictResolution::Manual,
            ConflictResolution::UseNewest,
            ConflictResolution::KeepBoth,
        ];

        for resolution in resolutions {
            let serialized = serde_json::to_string(&resolution).unwrap();
            let deserialized: ConflictResolution = serde_json::from_str(&serialized).unwrap();
            assert_eq!(resolution, deserialized);
        }
    }

    #[test]
    fn test_conflict_resolution_string_conversion() {
        assert_eq!(ConflictResolution::UseRemote.as_str(), "use_remote");
        assert_eq!(ConflictResolution::UseLocal.as_str(), "use_local");
        assert_eq!(ConflictResolution::Skip.as_str(), "skip");
        assert_eq!(ConflictResolution::Manual.as_str(), "manual");
        assert_eq!(ConflictResolution::UseNewest.as_str(), "use_newest");
        assert_eq!(ConflictResolution::KeepBoth.as_str(), "keep_both");

        // Test from_str
        assert_eq!(ConflictResolution::from_str("use_remote"), Some(ConflictResolution::UseRemote));
        assert_eq!(ConflictResolution::from_str("use_local"), Some(ConflictResolution::UseLocal));
        assert_eq!(ConflictResolution::from_str("skip"), Some(ConflictResolution::Skip));
        assert_eq!(ConflictResolution::from_str("manual"), Some(ConflictResolution::Manual));
        assert_eq!(ConflictResolution::from_str("use_newest"), Some(ConflictResolution::UseNewest));
        assert_eq!(ConflictResolution::from_str("keep_both"), Some(ConflictResolution::KeepBoth));
        assert_eq!(ConflictResolution::from_str("invalid"), None);
    }
}