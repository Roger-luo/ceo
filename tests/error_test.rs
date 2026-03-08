use ceo::error::{DbError, SyncError};
use std::path::PathBuf;

#[test]
fn db_error_not_found_displays_path_and_suggestion() {
    let err = DbError::NotFound(PathBuf::from("/tmp/ceo.db"));
    let msg = err.to_string();
    assert!(msg.contains("/tmp/ceo.db"), "should contain the path");
    assert!(msg.contains("ceo sync"), "should suggest running ceo sync");
}

#[test]
fn sync_error_from_db_error() {
    let db_err = DbError::NotFound(PathBuf::from("/tmp/ceo.db"));
    let sync_err: SyncError = db_err.into();
    assert!(sync_err.to_string().contains("ceo sync"));
}
