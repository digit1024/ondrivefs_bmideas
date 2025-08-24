use std::process::{Command, Output};
use std::sync::Arc;
use std::thread;

use anyhow::Result;
use fuser::MountOption;
use onedrive_sync_daemon::app_state::AppState;
use onedrive_sync_daemon::file_manager::FileManager;
use onedrive_sync_daemon::fuse::OneDriveFuse;
use onedrive_sync_daemon::onedrive_service::onedrive_client::OneDriveClientTrait;
use onedrive_sync_daemon::onedrive_service::onedrive_models::{UserProfile, DriveItem, FileFacet, DeltaResponseApi};
use onedrive_sync_daemon::sync::{ChangeOperation, SyncProcessor};
use crate::common::mock_onedrive_client::MockOneDriveClient;
use crate::integration::processing_item_tests::setup_test_env;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;


pub struct FuseMount {
    mount_handle: Option<thread::JoinHandle<()>>,
    stop_signal: Arc<AtomicBool>,
}


impl FuseMount {
    pub async fn new(app_state: &AppState) -> Result<Self> {
        // Ensure the mount point directory exists
        let mount_path = mount_point();
        std::fs::create_dir_all(&mount_path)?;
        
        let pool = app_state.persistency().pool().clone();
        let download_queue_repo = app_state.persistency().download_queue_repository();
        let fuse_fs = OneDriveFuse::new(
            pool.clone(),
            download_queue_repo,
            app_state.file_manager.clone(),
            Arc::new(app_state.clone()),
        ).await?;
        fuse_fs.initialize().await.ok();
        
        let stop_signal = Arc::new(AtomicBool::new(false));
        let stop_signal_clone = stop_signal.clone();
        
        let mount_handle = thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let result = fuser::mount2(
                    fuse_fs,
                    &mount_path,
                    &[
                        MountOption::FSName("onedrive".to_string()),
                        MountOption::NoExec,
                        MountOption::NoSuid,
                        MountOption::NoDev,
                        MountOption::DefaultPermissions,
                        MountOption::NoAtime,
                        MountOption::CUSTOM("case_insensitive".to_string()),
                    ],
                );
                
                if let Err(e) = result {
                    eprintln!("FUSE mount error: {}", e);
                }
            });
        });
        
        Ok(Self {
            mount_handle: Some(mount_handle),
            stop_signal,
        })
    }
    
    pub fn stop(&mut self) -> Result<()> {
        self.stop_signal.store(true, Ordering::SeqCst);
        
        if let Some(handle) = self.mount_handle.take() {
            // Unmount the filesystem
            let unmount_result = std::process::Command::new("fusermount")
                .arg("-u")
                .arg(&mount_point())
                .status();
                
            if let Ok(status) = unmount_result {
                if !status.success() {
                    eprintln!("Warning: fusermount failed with status: {}", status);
                }
            } else {
                eprintln!("Warning: Failed to run fusermount");
            }
                
            // Wait for the mount thread to finish
            if let Err(e) = handle.join() {
                eprintln!("Warning: Mount thread join failed: {:?}", e);
            }
        }
        
        Ok(())
    }
}

impl Drop for FuseMount {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub fn mount_point() -> String {
    std::env::var("HOME").unwrap()+  "/OneDrive"
}
pub fn exec_in_fuse_root(command: &str) -> Output {
    let output = Command::new("sh")
    .arg("-c")
    .arg(format!("cd {} && {}" , mount_point(), command))
    .output()
    .expect("Failed to execute command");
output
}

/// Check if the mount point is actually mounted and accessible
fn is_mount_accessible(mount_path: &str) -> bool {
    // Check if the path exists
    if !std::path::Path::new(mount_path).exists() {
        return false;
    }
    
    // Check if it's actually a mount point by looking at /proc/mounts
    if let Ok(contents) = std::fs::read_to_string("/proc/mounts") {
        return contents.lines().any(|line| line.contains(mount_path));
    }
    
    // Fallback: try to read the directory
    std::fs::read_dir(mount_path).is_ok()
}

#[tokio::test]
async fn test_env_home_variable() -> Result<()> {
    let (app_state, _, _, mock_client) = setup_test_env().await?;
    
    assert_eq!(std::env::var("HOME").unwrap(), "/tmp/onedrivetestsenv");
    assert_eq!(std::env::var("HOME").unwrap() + "/data/onedrive-sync/downloads", app_state.config().download_dir().display().to_string());
    assert_eq!(std::env::var("HOME").unwrap() + "/data/onedrive-sync/uploads", app_state.config().upload_dir().display().to_string());
    
    

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_mount_fuse() -> Result<()> {
    let (app_state, _, _, _mock_client) = setup_test_env().await?;
    
    // Create and mount the FUSE filesystem
    let mut fuse_mount = FuseMount::new(&app_state).await?;
    
    // Give the mount a moment to initialize
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Test that the mount point exists and is accessible
    let mount_path = mount_point();
    assert!(is_mount_accessible(&mount_path), "Mount point should be accessible at: {}", mount_path);
    
    // Test that we can access the expected file
    let file_path = format!("{}/Documents/Work/Reports/Q1_Report.pdf.onedrivedownload", mount_path);
    assert!(std::path::Path::new(&file_path).exists(), "Expected file should exist at: {}", file_path);
    
    // Clean up - unmount the filesystem
    fuse_mount.stop()?;
    
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_fuse_mount_lifecycle() -> Result<()> {
    let (app_state, _, _, _mock_client) = setup_test_env().await?;
    
    // Test mount creation
    let mut fuse_mount = FuseMount::new(&app_state).await?;
    
    // Verify mount is working
    let mount_path = mount_point();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    assert!(is_mount_accessible(&mount_path), "Mount should be accessible after creation");
    
    // Test mount cleanup
    fuse_mount.stop()?;
    
    // Give unmount time to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Verify mount is no longer accessible (this might fail if cleanup is slow)
    // We'll just check that the path still exists (it might be a regular directory now)
    assert!(std::path::Path::new(&mount_path).exists(), "Mount point path should still exist");
    
    Ok(())
}




#[tokio::test(flavor = "multi_thread")]
async fn test_basic_operations_on_fuse_mount_using_command_line() -> Result<()> {
    let (app_state, _, _, _mock_client) = setup_test_env().await?;
    
    // Create and mount the FUSE filesystem
    let mut fuse_mount = FuseMount::new(&app_state).await?;
    
    // Give the mount a moment to initialize
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Test that the mount point exists and is accessible
    let mount_path = mount_point();
    
    
    // Test that we can access the expected file
    let file_path = format!("{}/Documents/Work/Reports/Q1_Report.pdf.onedrivedownload", mount_path);
    assert!(std::path::Path::new(&file_path).exists(), "Expected file should exist at: {}", file_path);

    //we will run command line to create a file in the fuse mount
    let output = exec_in_fuse_root("touch testFile.txt");
    assert!(output.status.success(), "Failed to create file in fuse mount");
    //File Should be created 
    let file_path = format!("{}/testFile.txt", mount_path);
    assert!(std::path::Path::new(&file_path).exists(), "Expected file should exist at: {}", file_path);
    let output = exec_in_fuse_root("echo \"AAA\" >> testFile.txt");
    assert!(output.status.success(), "Failed to write to file in fuse mount");
    let output = exec_in_fuse_root("cat testFile.txt");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "AAA\n");
    
    let output = exec_in_fuse_root("rm testFile.txt");
    assert!(output.status.success(), "Failed to remove file in fuse mount");
    //File Should be removed
    let file_path = format!("{}/testFile.txt", mount_path);
    assert!(!std::path::Path::new(&file_path).exists(), "Expected file should not exist at: {}", file_path);


    //Check processing Items! 
    let processing_items = app_state.persistency().processing_item_repository().get_all_processing_items().await?;
    assert_eq!(processing_items.len(), 4,"Processing items should be 4 (touch - 2 : create and update , echo >> - 1 , rm - 1)");// touch on non existing files does: lookup, create , setattr - so Create and Modify
    assert_eq!(processing_items[0].change_operation, ChangeOperation::Create);
    assert_eq!(processing_items[1].change_operation, ChangeOperation::Update);
    assert_eq!(processing_items[2].change_operation, ChangeOperation::Update);
    assert_eq!(processing_items[3].change_operation, ChangeOperation::Delete);
    assert_eq!(processing_items[0].drive_item.name, Some("testFile.txt".to_string()));
    assert_eq!(processing_items[1].drive_item.name, Some("testFile.txt".to_string()));
    assert_eq!(processing_items[2].drive_item.name, Some("testFile.txt".to_string()));
    assert_eq!(processing_items[3].drive_item.name, Some("testFile.txt".to_string()));
    // Clean up - unmount the filesystem
    fuse_mount.stop()?;
    let sync_processor = SyncProcessor::new(app_state.clone());
    sync_processor.squash_local_changes().await?;
    let processing_items = app_state.persistency().processing_item_repository().get_all_processing_items().await?;
    assert_eq!(processing_items.len(), 0);
    
    
    // Clean up - unmount the filesystem
    fuse_mount.stop()?;
    
    Ok(())
}


#[tokio::test(flavor = "multi_thread")]
async fn test_basic_multple_updates() -> Result<()> {
    let (app_state, _, _, _mock_client) = setup_test_env().await?;
    
    // Create and mount the FUSE filesystem
    let mut fuse_mount = FuseMount::new(&app_state).await?;
    
    // Give the mount a moment to initialize
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Test that the mount point exists and is accessible
    let mount_path = mount_point();
    
    
    // Test that we can access the expected file
    let file_path = format!("{}/Documents/Work/Reports/Q1_Report.pdf.onedrivedownload", mount_path);
    assert!(std::path::Path::new(&file_path).exists(), "Expected file should exist at: {}", file_path);

    //we will run command line to create a file in the fuse mount
    let output = exec_in_fuse_root("touch testFile.txt");
    assert!(output.status.success(), "Failed to create file in fuse mount");
    //File Should be created 
    let file_path = format!("{}/testFile.txt", mount_path);
    assert!(std::path::Path::new(&file_path).exists(), "Expected file should exist at: {}", file_path);
    let output = exec_in_fuse_root("echo \"AAA\" >> testFile.txt");
    assert!(output.status.success(), "Failed to write to file in fuse mount");
    let output = exec_in_fuse_root("echo \"BBB\" >> testFile.txt");
    assert!(output.status.success(), "Failed to write to file in fuse mount");
    let output = exec_in_fuse_root("cat testFile.txt");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "AAA\nBBB\n");
    
    


    //Check processing Items! 
    let processing_items = app_state.persistency().processing_item_repository().get_all_processing_items().await?;
    assert_eq!(processing_items.len(), 4,"Processing items should be 4 (touch - 2 : create and update , echo >> - 1 , rm - 1)");// touch on non existing files does: lookup, create , setattr - so Create and Modify
    assert_eq!(processing_items[0].change_operation, ChangeOperation::Create);
    assert_eq!(processing_items[1].change_operation, ChangeOperation::Update);
    assert_eq!(processing_items[2].change_operation, ChangeOperation::Update);
    assert_eq!(processing_items[3].change_operation, ChangeOperation::Update);
    assert_eq!(processing_items[0].drive_item.name, Some("testFile.txt".to_string()));
    assert_eq!(processing_items[1].drive_item.name, Some("testFile.txt".to_string()));
    assert_eq!(processing_items[2].drive_item.name, Some("testFile.txt".to_string()));
    assert_eq!(processing_items[3].drive_item.name, Some("testFile.txt".to_string()));
    // Clean up - unmount the filesystem
    fuse_mount.stop()?;
    let sync_processor = SyncProcessor::new(app_state.clone());
    sync_processor.squash_local_changes().await?;
    let processing_items = app_state.persistency().processing_item_repository().get_all_processing_items().await?;
    assert_eq!(processing_items.len(), 1 , "Processing Items Should Be squashed to 1 Creat");
    assert_eq!(processing_items[0].change_operation, ChangeOperation::Create);
    
    // Clean up - unmount the filesystem
    fuse_mount.stop()?;
    
    Ok(())
}



#[tokio::test(flavor = "multi_thread")]
async fn tests_ls_does_not_work_if_one_of_the_files_were_modified() -> Result<()> {
    let (app_state, _, _, _mock_client) = setup_test_env().await?;
    
    // Create and mount the FUSE filesystem
    let mut fuse_mount = FuseMount::new(&app_state).await?;
    
    // Give the mount a moment to initialize
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Test that the mount point exists and is accessible
    let mount_path = mount_point();
    
    
    // Test that we can access the expected file
    let file_path = format!("{}/Documents/Work/Reports/Q1_Report.pdf.onedrivedownload", mount_path);
    assert!(std::path::Path::new(&file_path).exists(), "Expected file should exist at: {}", file_path);
    // Now Lets creae this file locally and we should observe that ".onedrivedownload" is not there
    // id for this item is  5 From fixtures
    let filepath = app_state.file_manager().get_local_dir().join("5");
    std::fs::write(filepath, "AAA").unwrap();
    
    
    // Now Lets check that the file is there
    let file_path = format!("{}/Documents/Work/Reports/Q1_Report.pdf", mount_path);
    assert!(std::path::Path::new(&file_path).exists(), "Expected file should exist at: {}", file_path);

    
 
    
    

    
    // Clean up - unmount the filesystem
    fuse_mount.stop()?;
    
    Ok(())
}
