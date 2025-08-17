//! ProfileRepository: Handles user_profiles table operations
use crate::onedrive_service::onedrive_models::UserProfile;
use anyhow::{Context, Result};
use log::info;
use sqlx::{Pool, Row, Sqlite};

/// Database operations for user profile
#[derive(Clone)]
pub struct ProfileRepository {
    pool: Pool<Sqlite>,
}

impl ProfileRepository {
    /// Create a new profile repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Store user profile (always overwrites - only one record)
    pub async fn store_profile(&self, profile: &UserProfile) -> Result<()> {
        // First, clear any existing profile records
        sqlx::query("DELETE FROM user_profiles")
            .execute(&self.pool)
            .await?;

        // Insert the new profile
        sqlx::query(
            r#"
            INSERT INTO user_profiles (
                id, display_name, given_name, surname, mail, user_principal_name,
                job_title, business_phones, mobile_phone, office_location, preferred_language
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&profile.id)
        .bind(&profile.display_name)
        .bind(&profile.given_name)
        .bind(&profile.surname)
        .bind(&profile.mail)
        .bind(&profile.user_principal_name)
        .bind(&profile.job_title)
        .bind(
            profile
                .business_phones
                .as_ref()
                .map(|phones| phones.join(",")),
        )
        .bind(&profile.mobile_phone)
        .bind(&profile.office_location)
        .bind(&profile.preferred_language)
        .execute(&self.pool)
        .await?;

        info!(
            "Stored user profile for: {}",
            profile.display_name.as_deref().unwrap_or("Unknown")
        );
        Ok(())
    }

    /// Get the stored user profile
    pub async fn get_profile(&self) -> Result<Option<UserProfile>> {
        let row = sqlx::query(
            r#"
            SELECT id, display_name, given_name, surname, mail, user_principal_name,
                   job_title, business_phones, mobile_phone, office_location, preferred_language
            FROM user_profiles LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let id: String = row.try_get("id")?;
            let display_name: Option<String> = row.try_get("display_name")?;
            let given_name: Option<String> = row.try_get("given_name")?;
            let surname: Option<String> = row.try_get("surname")?;
            let mail: Option<String> = row.try_get("mail")?;
            let user_principal_name: Option<String> = row.try_get("user_principal_name")?;
            let job_title: Option<String> = row.try_get("job_title")?;
            let business_phones_str: Option<String> = row.try_get("business_phones")?;
            let mobile_phone: Option<String> = row.try_get("mobile_phone")?;
            let office_location: Option<String> = row.try_get("office_location")?;
            let preferred_language: Option<String> = row.try_get("preferred_language")?;

            // Parse business phones from comma-separated string
            let business_phones = business_phones_str.map(|phones_str| {
                phones_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect()
            });

            let profile = UserProfile {
                id,
                display_name,
                given_name,
                surname,
                mail,
                user_principal_name,
                job_title,
                business_phones,
                mobile_phone,
                office_location,
                preferred_language,
            };

            Ok(Some(profile))
        } else {
            Ok(None)
        }
    }

    /// Clear the stored user profile
    pub async fn clear_profile(&self) -> Result<()> {
        sqlx::query("DELETE FROM user_profiles")
            .execute(&self.pool)
            .await?;

        info!("Cleared stored user profile");
        Ok(())
    }
}
