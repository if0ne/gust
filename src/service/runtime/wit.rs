// Scaffolding for the component/extension runtime; not all of this surface is
// wired into the executor yet.
#![allow(dead_code)]

use std::{
    collections::{BTreeSet, HashSet},
    fmt::Display,
    str::FromStr,
};

/// Describes the imports and exports of a WebAssembly component world.
#[derive(Clone, Debug, Default)]
pub(crate) struct WitWorld {
    pub imports: HashSet<WitInterface>,
    pub exports: HashSet<WitInterface>,
}

/// A parsed WIT interface identifier with namespace, package, interfaces, and optional version.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WitInterface {
    pub namespace: String,
    pub package: String,
    pub interfaces: BTreeSet<String>,
    pub version: Option<semver::Version>,
}

impl Display for WitInterface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.namespace, self.package)?;
        if !self.interfaces.is_empty() {
            write!(f, "/")?;
            let mut first = true;
            for iface in &self.interfaces {
                if !first {
                    write!(f, ",")?;
                }
                write!(f, "{}", iface)?;
                first = false;
            }
        }
        if let Some(v) = &self.version {
            write!(f, "@{}", v)?;
        }
        Ok(())
    }
}

impl FromStr for WitInterface {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (main, version) = match s.split_once('@') {
            Some((m, v)) => (m, Some(v)),
            None => (s, None),
        };

        let (namespace_package, interface) = match main.split_once('/') {
            Some((np, iface)) => (np, Some(iface)),
            None => (main, None),
        };

        let (namespace, package) = match namespace_package.split_once(':') {
            Some((ns, pkg)) => (ns, pkg),
            None => ("", namespace_package),
        };

        let interfaces = match interface {
            Some(iface) => iface
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|v| v.to_string())
                .collect(),
            None => BTreeSet::new(),
        };

        let version = version.and_then(|v| semver::Version::parse(v).ok());

        Ok(WitInterface {
            namespace: namespace.to_string(),
            package: package.to_string(),
            interfaces,
            version,
        })
    }
}

impl WitInterface {
    /// Returns the fully qualified package identifier (`namespace:package`).
    pub(crate) fn id(&self) -> super::types::InterfaceKey {
        super::types::InterfaceKey {
            namespace: self.namespace.clone(),
            package: self.package.clone(),
        }
    }

    /// Returns a semver version requirement derived from the interface version, if present.
    pub(crate) fn version_req(&self) -> Option<semver::VersionReq> {
        self.version
            .as_ref()
            .and_then(|v| semver::VersionReq::parse(&format!("^{}", v)).ok())
    }

    /// Returns the fully qualified names of all interfaces in this package.
    pub(crate) fn interfaces(&self) -> impl Iterator<Item = String> {
        self.interfaces
            .iter()
            .map(|iface| format!("{}:{}/{iface}", self.namespace, self.package))
    }

    /// Computes the intersection of two interfaces from the same package.
    pub(crate) fn intersect(&self, other: &WitInterface) -> Option<WitInterface> {
        if self.namespace != other.namespace || self.package != other.package {
            return None;
        }

        let version_match = match (&self.version, &other.version) {
            (Some(v1), Some(v2)) => {
                let req1 = self.version_req();
                let req2 = other.version_req();
                match (req1, req2) {
                    (Some(r1), Some(r2)) => r1.matches(v2) || r2.matches(v1),
                    (None, Some(_)) | (Some(_), None) | (None, None) => false,
                }
            }
            (None, None) => true,
            (Some(_), None) | (None, Some(_)) => false,
        };

        if !version_match {
            return None;
        }

        let common_interfaces: BTreeSet<String> = self
            .interfaces
            .intersection(&other.interfaces)
            .cloned()
            .collect();

        if common_interfaces.is_empty() {
            return None;
        }

        Some(Self {
            namespace: self.namespace.clone(),
            package: self.package.clone(),
            interfaces: common_interfaces,
            version: self.version.clone(),
        })
    }

    /// Merges two interfaces from the same package, combining their interface sets.
    pub(crate) fn merge(&self, other: &WitInterface) -> anyhow::Result<WitInterface> {
        anyhow::ensure!(
            self.namespace == other.namespace,
            "interfaces have different namespaces"
        );

        anyhow::ensure!(
            self.package == other.package,
            "interfaces have different packages"
        );

        let interfaces = self
            .interfaces
            .union(&other.interfaces)
            .cloned()
            .collect::<BTreeSet<_>>();

        let version = match (&self.version, &other.version) {
            (Some(v1), Some(v2)) => Some(v1.max(v2)),
            (Some(v1), None) => Some(v1),
            (None, Some(v2)) => Some(v2),
            (None, None) => None,
        }
        .cloned();

        Ok(WitInterface {
            namespace: self.namespace.clone(),
            package: self.package.clone(),
            interfaces,
            version,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, str::FromStr};

    use super::*;

    #[test]
    fn comparator() {
        let component_wit = WitInterface::from_str("wasi:http/incoming-handler@0.2.4").unwrap();
        let extension_wit = WitInterface::from_str("wasi:http/incoming-handler@0.2.6").unwrap();

        assert!(component_wit.intersect(&extension_wit).is_some());
        assert!(extension_wit.intersect(&component_wit).is_some());
    }

    #[test]
    fn parse_full_format() {
        let wit = WitInterface::from_str("wasi:http/incoming-handler@0.2.4").unwrap();
        assert_eq!(wit.namespace, "wasi");
        assert_eq!(wit.package, "http");
        assert_eq!(
            wit.interfaces,
            BTreeSet::from(["incoming-handler".to_string()])
        );
        assert_eq!(wit.version, Some(semver::Version::new(0, 2, 4)));
    }

    #[test]
    fn parse_no_version() {
        let wit = WitInterface::from_str("wasi:http/incoming-handler").unwrap();
        assert_eq!(wit.namespace, "wasi");
        assert_eq!(wit.package, "http");
        assert_eq!(
            wit.interfaces,
            BTreeSet::from(["incoming-handler".to_string()])
        );
        assert!(wit.version.is_none());
    }

    #[test]
    fn parse_no_interface() {
        let wit = WitInterface::from_str("wasi:http").unwrap();
        assert_eq!(wit.namespace, "wasi");
        assert_eq!(wit.package, "http");
        assert!(wit.interfaces.is_empty());
        assert!(wit.version.is_none());
    }

    #[test]
    fn parse_multiple_interfaces() {
        let wit = WitInterface::from_str("wasi:http/types,outgoing-handler@0.2.9").unwrap();
        assert_eq!(
            wit.interfaces,
            BTreeSet::from(["types".to_string(), "outgoing-handler".to_string()])
        );
        assert_eq!(wit.version, Some(semver::Version::new(0, 2, 9)));
    }

    #[test]
    fn parse_no_namespace() {
        let wit = WitInterface::from_str("http/handler").unwrap();
        assert_eq!(wit.namespace, "");
        assert_eq!(wit.package, "http");
        assert_eq!(wit.interfaces, BTreeSet::from(["handler".to_string()]));
    }

    #[test]
    fn parse_version_no_interface() {
        let wit = WitInterface::from_str("wasi:cli@0.2.6").unwrap();
        assert_eq!(wit.namespace, "wasi");
        assert_eq!(wit.package, "cli");
        assert!(wit.interfaces.is_empty());
        assert_eq!(wit.version, Some(semver::Version::new(0, 2, 6)));
    }

    #[test]
    fn parse_invalid_version_ignored() {
        let wit = WitInterface::from_str("wasi:http/handler@not-a-version").unwrap();
        assert!(wit.version.is_none());
    }

    #[test]
    fn display_full() {
        let wit = WitInterface::from_str("wasi:http/incoming-handler@0.2.4").unwrap();
        assert_eq!(wit.to_string(), "wasi:http/incoming-handler@0.2.4");
    }

    #[test]
    fn display_no_version() {
        let wit = WitInterface::from_str("wasi:http/handler").unwrap();
        assert_eq!(wit.to_string(), "wasi:http/handler");
    }

    #[test]
    fn display_no_interfaces() {
        let wit = WitInterface::from_str("wasi:http").unwrap();
        assert_eq!(wit.to_string(), "wasi:http");
    }

    #[test]
    fn display_multiple_interfaces_sorted() {
        let wit = WitInterface {
            namespace: "wasi".to_string(),
            package: "http".to_string(),
            interfaces: BTreeSet::from(["types".to_string(), "outgoing-handler".to_string()]),
            version: Some(semver::Version::new(0, 2, 9)),
        };
        assert_eq!(wit.to_string(), "wasi:http/outgoing-handler,types@0.2.9");
    }

    #[test]
    fn version_req_caret_semantics() {
        let wit = WitInterface::from_str("wasi:http/handler@0.2.4").unwrap();
        let req = wit.version_req().unwrap();
        assert!(req.matches(&semver::Version::new(0, 2, 4)));
        assert!(req.matches(&semver::Version::new(0, 2, 9)));
        assert!(!req.matches(&semver::Version::new(0, 3, 0)));
    }

    #[test]
    fn version_req_none_when_no_version() {
        let wit = WitInterface::from_str("wasi:http/handler").unwrap();
        assert!(wit.version_req().is_none());
    }

    #[test]
    fn interfaces_fully_qualified() {
        let wit = WitInterface::from_str("wasi:http/types,handler@0.2.9").unwrap();
        let mut names: Vec<String> = wit.interfaces().collect();
        names.sort();
        assert_eq!(names, vec!["wasi:http/handler", "wasi:http/types"]);
    }

    #[test]
    fn interfaces_empty_when_none() {
        let wit = WitInterface::from_str("wasi:http").unwrap();
        assert_eq!(wit.interfaces().count(), 0);
    }

    #[test]
    fn intersect_compatible_minor_versions() {
        let a = WitInterface::from_str("wasi:http/handler@0.2.4").unwrap();
        let b = WitInterface::from_str("wasi:http/handler@0.2.6").unwrap();
        let result = a.intersect(&b).unwrap();
        assert_eq!(result.interfaces, BTreeSet::from(["handler".to_string()]));
        assert_eq!(result.version, Some(semver::Version::new(0, 2, 4)));
    }

    #[test]
    fn intersect_different_namespace_returns_none() {
        let a = WitInterface::from_str("wasi:http/handler@0.2.4").unwrap();
        let b = WitInterface::from_str("other:http/handler@0.2.4").unwrap();
        assert!(a.intersect(&b).is_none());
    }

    #[test]
    fn intersect_different_package_returns_none() {
        let a = WitInterface::from_str("wasi:http/handler@0.2.4").unwrap();
        let b = WitInterface::from_str("wasi:cli/handler@0.2.4").unwrap();
        assert!(a.intersect(&b).is_none());
    }

    #[test]
    fn intersect_incompatible_versions_returns_none() {
        let a = WitInterface::from_str("wasi:http/handler@0.2.4").unwrap();
        let b = WitInterface::from_str("wasi:http/handler@0.3.0").unwrap();
        assert!(a.intersect(&b).is_none());
    }

    #[test]
    fn intersect_no_common_interfaces_returns_none() {
        let a = WitInterface::from_str("wasi:http/types@0.2.4").unwrap();
        let b = WitInterface::from_str("wasi:http/handler@0.2.4").unwrap();
        assert!(a.intersect(&b).is_none());
    }

    #[test]
    fn intersect_partial_interface_overlap() {
        let a = WitInterface {
            namespace: "wasi".to_string(),
            package: "http".to_string(),
            interfaces: BTreeSet::from(["types".to_string(), "handler".to_string()]),
            version: Some(semver::Version::new(0, 2, 4)),
        };
        let b = WitInterface {
            namespace: "wasi".to_string(),
            package: "http".to_string(),
            interfaces: BTreeSet::from(["types".to_string(), "outgoing".to_string()]),
            version: Some(semver::Version::new(0, 2, 6)),
        };
        let result = a.intersect(&b).unwrap();
        assert_eq!(result.interfaces, BTreeSet::from(["types".to_string()]));
    }

    #[test]
    fn intersect_both_no_version() {
        let a = WitInterface::from_str("wasi:http/handler").unwrap();
        let b = WitInterface::from_str("wasi:http/handler").unwrap();
        let result = a.intersect(&b).unwrap();
        assert_eq!(result.interfaces, BTreeSet::from(["handler".to_string()]));
        assert!(result.version.is_none());
    }

    #[test]
    fn intersect_one_versioned_one_not_returns_none() {
        let a = WitInterface::from_str("wasi:http/handler@0.2.4").unwrap();
        let b = WitInterface::from_str("wasi:http/handler").unwrap();
        assert!(a.intersect(&b).is_none());
        assert!(b.intersect(&a).is_none());
    }

    #[test]
    fn merge_combines_interfaces() {
        let a = WitInterface::from_str("wasi:http/types@0.2.4").unwrap();
        let b = WitInterface::from_str("wasi:http/handler@0.2.6").unwrap();
        let result = a.merge(&b).unwrap();
        assert_eq!(
            result.interfaces,
            BTreeSet::from(["types".to_string(), "handler".to_string()])
        );
    }

    #[test]
    fn merge_takes_max_version() {
        let a = WitInterface::from_str("wasi:http/types@0.2.4").unwrap();
        let b = WitInterface::from_str("wasi:http/handler@0.2.6").unwrap();
        let result = a.merge(&b).unwrap();
        assert_eq!(result.version, Some(semver::Version::new(0, 2, 6)));
    }

    #[test]
    fn merge_different_namespace_fails() {
        let a = WitInterface::from_str("wasi:http/types").unwrap();
        let b = WitInterface::from_str("other:http/types").unwrap();
        assert!(a.merge(&b).is_err());
    }

    #[test]
    fn merge_different_package_fails() {
        let a = WitInterface::from_str("wasi:http/types").unwrap();
        let b = WitInterface::from_str("wasi:cli/types").unwrap();
        assert!(a.merge(&b).is_err());
    }

    #[test]
    fn merge_one_version_none() {
        let a = WitInterface::from_str("wasi:http/types@0.2.4").unwrap();
        let b = WitInterface::from_str("wasi:http/handler").unwrap();
        let result = a.merge(&b).unwrap();
        assert_eq!(result.version, Some(semver::Version::new(0, 2, 4)));
    }

    #[test]
    fn merge_both_none_version() {
        let a = WitInterface::from_str("wasi:http/types").unwrap();
        let b = WitInterface::from_str("wasi:http/handler").unwrap();
        let result = a.merge(&b).unwrap();
        assert!(result.version.is_none());
    }

    #[test]
    fn merge_overlapping_interfaces_deduplicates() {
        let a = WitInterface {
            namespace: "wasi".to_string(),
            package: "http".to_string(),
            interfaces: BTreeSet::from(["types".to_string(), "handler".to_string()]),
            version: None,
        };
        let b = WitInterface {
            namespace: "wasi".to_string(),
            package: "http".to_string(),
            interfaces: BTreeSet::from(["types".to_string(), "outgoing".to_string()]),
            version: None,
        };
        let result = a.merge(&b).unwrap();
        assert_eq!(
            result.interfaces,
            BTreeSet::from([
                "types".to_string(),
                "handler".to_string(),
                "outgoing".to_string()
            ])
        );
    }

    #[test]
    fn wit_world_default_is_empty() {
        let world = WitWorld::default();
        assert!(world.imports.is_empty());
        assert!(world.exports.is_empty());
    }
}
