use matrix_sdk::{
    Client,
    encryption::{CrossSigningResetAuthType, VerificationState},
    ruma::api::client::uiaa,
};
use serde::Serialize;
use tracing::{info, warn};

pub(crate) async fn ensure_cross_signing_bootstrap(
    account_id: &str,
    matrix_password: &str,
    client: &Client,
) -> anyhow::Result<()> {
    let Some(user_id) = client.user_id() else {
        anyhow::bail!("logged-in Matrix client has no user id");
    };

    let encryption = client.encryption();

    if let Err(error) = encryption.bootstrap_cross_signing_if_needed(None).await {
        if let Some(response) = error.as_uiaa_response() {
            let mut password = uiaa::Password::new(
                uiaa::UserIdentifier::Matrix(uiaa::MatrixUserIdentifier::new(user_id.to_string())),
                matrix_password.to_owned(),
            );
            password.session = response.session.clone();

            encryption
                .bootstrap_cross_signing_if_needed(Some(uiaa::AuthData::Password(password)))
                .await?;
        } else {
            return Err(error.into());
        }
    }

    let status = build_device_status_response(client).await?;

    if status.has_cross_signing_identity && !status.is_device_verified {
        warn!(
            account = %account_id,
            user = %status.user_id,
            device_id = %status.device_id,
            verification_state = %status.verification_state,
            is_device_verified = status.is_device_verified,
            has_cross_signing_identity = status.has_cross_signing_identity,
            cross_signing_identity_verified = ?status.cross_signing_identity_verified,
            "gateway device is still not verified; complete a one-time manual verification if this account already had cross-signing set up elsewhere"
        );
    } else {
        info!(
            account = %account_id,
            user = %status.user_id,
            device_id = %status.device_id,
            verification_state = %status.verification_state,
            is_device_verified = status.is_device_verified,
            has_cross_signing_identity = status.has_cross_signing_identity,
            cross_signing_identity_verified = ?status.cross_signing_identity_verified,
            "initialized Matrix device trust state"
        );
    }

    Ok(())
}

pub(crate) async fn reset_cross_signing(
    account_id: &str,
    matrix_password: &str,
    client: &Client,
) -> anyhow::Result<DeviceStatusResponse> {
    let Some(user_id) = client.user_id() else {
        anyhow::bail!("logged-in Matrix client has no user id");
    };

    if let Some(handle) = client.encryption().reset_cross_signing().await? {
        match handle.auth_type() {
            CrossSigningResetAuthType::Uiaa(uiaa) => {
                let mut password = uiaa::Password::new(
                    uiaa::UserIdentifier::Matrix(uiaa::MatrixUserIdentifier::new(
                        user_id.to_string(),
                    )),
                    matrix_password.to_owned(),
                );
                password.session = uiaa.session.clone();

                handle
                    .auth(Some(uiaa::AuthData::Password(password)))
                    .await?;
            }
            CrossSigningResetAuthType::OAuth(oauth) => {
                anyhow::bail!(
                    "homeserver requires OAuth approval at {} to reset cross-signing",
                    oauth.approval_url
                );
            }
        }
    }

    let status = build_device_status_response(client).await?;

    info!(
        account = %account_id,
        user = %status.user_id,
        device_id = %status.device_id,
        verification_state = %status.verification_state,
        is_device_verified = status.is_device_verified,
        has_cross_signing_identity = status.has_cross_signing_identity,
        cross_signing_identity_verified = ?status.cross_signing_identity_verified,
        "reset Matrix cross-signing identity"
    );

    Ok(status)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceStatusResponse {
    pub(crate) user_id: String,
    pub(crate) device_id: String,
    pub(crate) verification_state: String,
    pub(crate) is_device_verified: bool,
    pub(crate) has_cross_signing_identity: bool,
    pub(crate) cross_signing_identity_verified: Option<bool>,
}

async fn build_device_status_response(client: &Client) -> anyhow::Result<DeviceStatusResponse> {
    let Some(user_id) = client.user_id() else {
        anyhow::bail!("logged-in Matrix client has no user id");
    };
    let Some(device_id) = client.device_id() else {
        anyhow::bail!("logged-in Matrix client has no device id");
    };

    let user_identity = client.encryption().request_user_identity(user_id).await?;
    let verification_state = client.encryption().verification_state().get();
    let own_device = client
        .encryption()
        .get_own_device()
        .await?
        .ok_or_else(|| anyhow::anyhow!("logged-in Matrix client has no local device"))?;

    Ok(DeviceStatusResponse {
        user_id: user_id.to_string(),
        device_id: device_id.to_string(),
        verification_state: verification_state_label(verification_state).to_owned(),
        is_device_verified: own_device.is_verified(),
        has_cross_signing_identity: user_identity.is_some(),
        cross_signing_identity_verified: user_identity.map(|identity| identity.is_verified()),
    })
}

fn verification_state_label(state: VerificationState) -> &'static str {
    match state {
        VerificationState::Unknown => "unknown",
        VerificationState::Verified => "verified",
        VerificationState::Unverified => "unverified",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_state_labels_are_stable() {
        assert_eq!(
            verification_state_label(VerificationState::Unknown),
            "unknown"
        );
        assert_eq!(
            verification_state_label(VerificationState::Verified),
            "verified"
        );
        assert_eq!(
            verification_state_label(VerificationState::Unverified),
            "unverified"
        );
    }

    #[test]
    fn device_status_response_serializes_as_camel_case() {
        let response = DeviceStatusResponse {
            user_id: "@bot:example.org".to_owned(),
            device_id: "DEVICE123".to_owned(),
            verification_state: "verified".to_owned(),
            is_device_verified: true,
            has_cross_signing_identity: true,
            cross_signing_identity_verified: Some(true),
        };

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            serde_json::json!({
                "userId": "@bot:example.org",
                "deviceId": "DEVICE123",
                "verificationState": "verified",
                "isDeviceVerified": true,
                "hasCrossSigningIdentity": true,
                "crossSigningIdentityVerified": true,
            })
        );
    }
}
