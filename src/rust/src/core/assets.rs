/*
 * Copyright 2026 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

use std::{
    collections::{BTreeSet, HashMap},
    fs,
    sync::Arc,
};

use sha2::{Digest, Sha512};
use thiserror::Error;

use crate::{
    common::SemanticVersion, core::call_rwlock::CallRwLock, protobuf::assets::AssetMetadata,
};

/// Asset loading and verification errors
#[derive(Error, Debug, PartialEq, Eq)]
pub enum AssetError {
    #[error("unknown asset_group {0}")]
    UnsupportedAsset(String),
    #[error("failed to read asset file '{0}' due to '{1}'")]
    FailedToReadAssetFile(String, String),
    #[error("failed to access asset registry due to '{0}'")]
    FailedToAccessAssetRegistry(String),
    #[error("Invalid asset metadata provided {0}")]
    InvalidAssetMetadata(String),
    #[error("no matching asset for asset_group={asset_group} with hash={content_hash}")]
    InvalidAssetPayload {
        asset_group: String,
        content_hash: String,
    },

    #[error("Asset already registered with version '{0}' for asset group '{1}'")]
    AssetAlreadyRegistered(SemanticVersion, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    version: SemanticVersion,
    metadata: AssetMetadata,
    content: Arc<Vec<u8>>,
}

impl Asset {
    pub fn new(metadata: AssetMetadata, content: Vec<u8>) -> Result<Self, AssetError> {
        let version = metadata.version.as_str().try_into().map_err(|_| {
            AssetError::InvalidAssetMetadata(format!("version={}", metadata.version))
        })?;
        Ok(Self {
            version,
            metadata,
            content: Arc::new(content),
        })
    }
}

impl Ord for Asset {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.metadata.cmp(&other.metadata) {
            core::cmp::Ordering::Equal => {}
            ord => return ord,
        }

        self.version.cmp(&other.version)
    }
}

impl PartialOrd for Asset {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetHandle {
    FilePath(String),
    Content(Vec<u8>),
}

/// Manages assets. Supports verifying [AssetHandle]s against a supported asset manifest.
/// Looks up the potential asset matches by filtering by asset_Group.
/// Once verified, saves the asset to a registry.
#[derive(Debug)]
pub struct AssetManager {
    /// Metadata manifest used by the asset manager to verify added assets are supported.
    supported_assets: Vec<AssetMetadata>,
    registry: AssetRegistry,
}

impl AssetManager {
    pub fn new(supported_assets: Vec<AssetMetadata>) -> Self {
        Self {
            supported_assets,
            registry: AssetRegistry::default(),
        }
    }

    /// Loads [Asset] content by resolving [AssetHandle]. Verifies content against
    /// [AssetMetadata] in [AssetManager::supported_assets] manifest. Looks for a match
    /// against any [AssetMetadata] with the specified [asset_group].
    pub fn add_asset_for_feature(
        &mut self,
        asset_group: &str,
        handle: AssetHandle,
    ) -> Result<(), AssetError> {
        let supported_versions = self.supported_versions(asset_group);
        if supported_versions.is_empty() {
            return Err(AssetError::UnsupportedAsset(asset_group.to_string()));
        }
        let mut content = Self::resolve_handle(handle)?;
        let metadata =
            Self::verify_asset(asset_group, &supported_versions, &mut content)?.to_owned();
        self.registry
            .add_asset_for_feature(asset_group, metadata, content)?;
        Ok(())
    }

    /// Looks up assets by asset ID, and returns them in descending semantic version order.
    pub fn get_options_for(&self, asset_group: &str) -> Option<Vec<SemanticVersion>> {
        self.registry.get_options_for(asset_group)
    }

    /// Attempts to resolve [AssetHandle] to the asset's byte content.
    fn resolve_handle(handle: AssetHandle) -> Result<Vec<u8>, AssetError> {
        match handle {
            AssetHandle::FilePath(path) => match fs::read(&path) {
                Ok(content) => Ok(content),
                Err(e) => Err(AssetError::FailedToReadAssetFile(
                    path,
                    e.kind().to_string(),
                )),
            },
            AssetHandle::Content(content) => Ok(content),
        }
    }

    /// Gets all [AssetMetadata] related to [asset_group]
    fn supported_versions(&self, asset_group: &str) -> Vec<&AssetMetadata> {
        self.supported_assets
            .iter()
            .filter(|metadata| metadata.asset_group == asset_group)
            .collect()
    }

    fn verify_asset(
        asset_group: &str,
        supported_versions: &[&AssetMetadata],
        content: &mut Vec<u8>,
    ) -> Result<AssetMetadata, AssetError> {
        for metadata in supported_versions {
            let expected_size = metadata.size_bytes as usize;
            if content.len() < expected_size {
                continue;
            }

            let content_hash = Sha512::digest(&content[..expected_size]);
            if content_hash[..] == metadata.sha512_hash[..] {
                content.truncate(expected_size);
                return Ok((*metadata).clone());
            }
        }

        Err(AssetError::InvalidAssetPayload {
            asset_group: asset_group.to_string(),
            content_hash: hex::encode(Sha512::digest(content)),
        })
    }

    pub fn get_registry(&self) -> AssetRegistry {
        self.registry.clone()
    }
}

/// Provides readonly, copy-out access to assets.
/// Supports looking up available versions for an asset group.
#[derive(Clone, Debug)]
pub struct AssetRegistry {
    /// Registry storing verified assets. Organized by asset_group then by AssetMetadata.
    assets: Arc<CallRwLock<HashMap<String, BTreeSet<Asset>>>>,
}

impl Default for AssetRegistry {
    fn default() -> Self {
        AssetRegistry::new(HashMap::new())
    }
}

impl AssetRegistry {
    pub fn new(assets: HashMap<String, BTreeSet<Asset>>) -> Self {
        AssetRegistry {
            assets: Arc::new(CallRwLock::new(assets, "asset-registry")),
        }
    }

    /// Looks up assets by asset ID, and returns them in descending semantic version order.
    pub fn get_options_for(&self, asset_group: &str) -> Option<Vec<SemanticVersion>> {
        match self.assets.read() {
            Err(e) => {
                error!("Failed to read asset registry due to lock: {e:?}");
                None
            }
            Ok(set) => set
                .get(asset_group)
                .map(|set| set.iter().map(|a| a.version).collect()),
        }
    }

    /// Looks up and returns a clone of an asset.
    pub fn get_asset(&self, asset_group: &str, version: SemanticVersion) -> Option<Asset> {
        match self.assets.read() {
            Err(e) => {
                error!("Failed to read asset registry due to lock: {e:?}");
                None
            }
            Ok(set) => set
                .get(asset_group)
                .and_then(|set| set.iter().find(|a| a.version == version).cloned()),
        }
    }

    fn add_asset_for_feature(
        &self,
        asset_group: &str,
        metadata: AssetMetadata,
        content: Vec<u8>,
    ) -> Result<(), AssetError> {
        let mut assets = self
            .assets
            .write()
            .map_err(|e| AssetError::FailedToAccessAssetRegistry(format!("{e:?}")))?;
        let asset = Asset::new(metadata, content)?;
        let version = asset.version;
        let asset_set = assets
            .entry(asset_group.to_owned())
            .or_insert(BTreeSet::default());

        if asset_set.iter().any(|a| a.version == version) {
            return Err(AssetError::AssetAlreadyRegistered(
                version,
                asset_group.to_owned(),
            ));
        }
        let _ = asset_set.insert(asset);

        Ok(())
    }
}

#[cfg(feature = "sim")]
pub mod manifest {
    use crate::protobuf::assets::AssetMetadata;

    pub fn supported_assets() -> Vec<AssetMetadata> {
        vec![]
    }
}

#[cfg(not(feature = "sim"))]
pub mod manifest {
    use crate::protobuf::assets::AssetMetadata;

    pub fn supported_assets() -> Vec<AssetMetadata> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha512};

    use super::*;

    const TEST_ASSET_GROUP: &str = "test-asset";

    fn make_metadata(asset_group: &str, version: &str, content: &[u8]) -> AssetMetadata {
        AssetMetadata {
            asset_group: asset_group.to_string(),
            version: version.to_string(),
            size_bytes: content.len() as u32,
            sha512_hash: Sha512::digest(content).to_vec(),
        }
    }

    #[test]
    fn test_supported_versions() {
        let manager = AssetManager::new(vec![
            make_metadata(TEST_ASSET_GROUP, "0.0.1", b"content_a"),
            make_metadata(TEST_ASSET_GROUP, "0.0.2", b"content_b"),
            make_metadata("other-asset", "0.0.1", b"content_c"),
        ]);

        let versions = manager.supported_versions(TEST_ASSET_GROUP);
        assert_eq!(versions.len(), 2);
        assert!(versions.iter().all(|m| m.asset_group == TEST_ASSET_GROUP));

        let other = manager.supported_versions("other-asset");
        assert_eq!(other.len(), 1);

        let empty = manager.supported_versions("nonexistent");
        assert!(empty.is_empty());
    }

    #[test]
    fn test_add_unsupported_asset() {
        let mut manager = AssetManager::new(vec![make_metadata("other-asset", "0.0.1", b"data")]);

        let result =
            manager.add_asset_for_feature(TEST_ASSET_GROUP, AssetHandle::Content(b"data".to_vec()));

        assert!(matches!(
            result,
            Err(AssetError::UnsupportedAsset(id)) if id == TEST_ASSET_GROUP
        ));

        let registry = manager.get_registry();
        assert_eq!(
            registry.assets.read().expect("got lock").len(),
            0,
            "registry is empty"
        )
    }

    #[test]
    fn test_failed_to_read_asset_file() {
        let mut manager =
            AssetManager::new(vec![make_metadata(TEST_ASSET_GROUP, "0.0.1", b"data")]);

        let result = manager.add_asset_for_feature(
            TEST_ASSET_GROUP,
            AssetHandle::FilePath("/nonexistent/path/to/asset.bin".to_string()),
        );

        assert!(matches!(
            result,
            Err(AssetError::FailedToReadAssetFile(_, _))
        ));

        let registry = manager.get_registry();
        assert_eq!(
            registry.assets.read().expect("got lock").len(),
            0,
            "registry is empty"
        )
    }

    #[test]
    fn test_invalid_asset_payload() {
        let mut manager = AssetManager::new(vec![make_metadata(
            TEST_ASSET_GROUP,
            "0.0.1",
            b"valid_content",
        )]);

        let result = manager.add_asset_for_feature(
            TEST_ASSET_GROUP,
            AssetHandle::Content(b"wrong_content".to_vec()),
        );

        assert!(matches!(
            result,
            Err(AssetError::InvalidAssetPayload { .. })
        ));
        let registry = manager.get_registry();
        assert_eq!(
            registry.assets.read().expect("got lock").len(),
            0,
            "registry is empty"
        )
    }

    #[test]
    fn test_asset_already_registered() {
        let content = b"valid_content";
        let mut manager = AssetManager::new(vec![make_metadata(
            TEST_ASSET_GROUP,
            "0.0.1",
            content.as_slice(),
        )]);

        assert!(
            manager
                .add_asset_for_feature(TEST_ASSET_GROUP, AssetHandle::Content(content.to_vec()))
                .is_ok()
        );
        let result =
            manager.add_asset_for_feature(TEST_ASSET_GROUP, AssetHandle::Content(content.to_vec()));

        assert!(matches!(
            result,
            Err(AssetError::AssetAlreadyRegistered { .. })
        ));

        let registry = manager.get_registry();
        assert_eq!(registry.assets.read().expect("got lock").len(), 1)
    }

    #[test]
    fn test_add_asset_success() {
        let asset_1 = Asset {
            version: SemanticVersion::new(0, 0, 1),
            metadata: make_metadata(TEST_ASSET_GROUP, "0.0.1", b"content1"),
            content: Arc::new(b"content1".to_vec()),
        };
        let asset_2 = Asset {
            version: SemanticVersion::new(0, 0, 2),
            metadata: make_metadata(TEST_ASSET_GROUP, "0.0.2", b"content2"),
            content: Arc::new(b"content2".to_vec()),
        };

        let mut manager =
            AssetManager::new(vec![asset_1.metadata.clone(), asset_2.metadata.clone()]);

        manager
            .add_asset_for_feature(
                TEST_ASSET_GROUP,
                AssetHandle::Content(asset_1.content.to_vec()),
            )
            .expect("should succeed");

        let versions = manager
            .get_options_for(TEST_ASSET_GROUP)
            .expect("should exist");
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0], asset_1.version);

        let registry = manager.get_registry();
        assert_eq!(registry.assets.read().expect("got lock").len(), 1);

        manager
            .add_asset_for_feature(
                TEST_ASSET_GROUP,
                AssetHandle::Content(asset_2.content.to_vec()),
            )
            .expect("should succeed");

        let versions = registry
            .get_options_for(TEST_ASSET_GROUP)
            .expect("should exist");
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0], asset_1.version);
        assert_eq!(versions[1], asset_2.version);

        assert_eq!(
            asset_1,
            registry
                .get_asset(TEST_ASSET_GROUP, asset_1.version)
                .unwrap()
        );
        assert_eq!(
            asset_2,
            registry
                .get_asset(TEST_ASSET_GROUP, asset_2.version)
                .unwrap()
        );
    }
}
