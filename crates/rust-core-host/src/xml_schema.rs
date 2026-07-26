//! XML schema-profile keying (issue #63): Maven POM coordinate identity and
//! MSBuild Include-attribute identity, extracted from lib.rs verbatim (issue #29
//! monolith split, phase B). Consumed by the path-profile matching augmentation.

use std::collections::HashMap;

use crate::SemanticNode;

// ── XML schema profile: Maven POM (issue #63). A <dependency>/<plugin> is identified by its
// groupId+artifactId CHILD TEXT, not by position — <dependencies>/<plugins> are unordered keyed
// collections. Without this, a dependency reorder swallows a concurrent version bump entirely
// (gumtree pairs slf4j's subtree with guava's positionally and the 2.0.9→2.0.10 edit vanishes —
// the #46 disease). Descendants of a keyed element key under their owner's coordinate so
// version-under-slf4j can never pair with version-under-guava.

pub(crate) const POM_COORDINATE_TAGS: &[&str] = &["dependency", "plugin", "extension", "exclusion"];

pub(crate) fn xml_tree_is_pom(tree: &SemanticNode) -> bool {
    // Root document -> <project> with a POM-shaped child (modelVersion/dependencies/build).
    tree.children.iter().any(|top| {
        top.node_type == "element"
            && top.label == "project"
            && top.children.iter().any(|c| {
                matches!(
                    c.label.as_str(),
                    "modelVersion" | "dependencies" | "dependencyManagement" | "build"
                        | "groupId" | "artifactId" | "parent" | "properties"
                )
            })
    })
}

pub(crate) fn pom_child_element_text(node: &SemanticNode, tag: &str) -> Option<String> {
    let child = node
        .children
        .iter()
        .find(|c| c.node_type == "element" && c.label == tag)?;
    child
        .children
        .iter()
        .find(|t| t.node_type == "text" && !t.label.is_empty())
        .map(|t| t.label.clone())
}

/// The coordinate key for a POM dependency-like element, when both parts are present.
pub(crate) fn pom_coordinate_key(node: &SemanticNode) -> Option<Vec<String>> {
    if node.node_type != "element" || !POM_COORDINATE_TAGS.contains(&node.label.as_str()) {
        return None;
    }
    let group = pom_child_element_text(node, "groupId")?;
    let artifact = pom_child_element_text(node, "artifactId")?;
    Some(vec![
        "xml".to_string(),
        "pom".to_string(),
        node.label.clone(),
        group,
        artifact,
    ])
}

// ── XML schema profile: MSBuild (issue #63). Item elements (<PackageReference .../>) are
// identified by their Include ATTRIBUTE — ItemGroup is an unordered keyed collection. Without
// this a reorder+version-bump degrades to MOVE churn plus a DELETE+ADD of the version attribute.

pub(crate) const MSBUILD_INCLUDE_TAGS: &[&str] = &[
    "PackageReference",
    "ProjectReference",
    "FrameworkReference",
    "Reference",
    "Compile",
    "Content",
    "EmbeddedResource",
    "None",
    "Using",
];

pub(crate) fn xml_tree_is_msbuild(tree: &SemanticNode) -> bool {
    tree.children.iter().any(|top| {
        top.node_type == "element"
            && top.label == "Project"
            && top.children.iter().any(|c| {
                (c.node_type == "attribute" && c.label.starts_with("sdk="))
                    || (c.node_type == "element"
                        && matches!(c.label.as_str(), "ItemGroup" | "PropertyGroup" | "Target"))
            })
    })
}

/// The Include-attribute key for an MSBuild item element.
pub(crate) fn msbuild_coordinate_key(node: &SemanticNode) -> Option<Vec<String>> {
    if node.node_type != "element" || !MSBUILD_INCLUDE_TAGS.contains(&node.label.as_str()) {
        return None;
    }
    let include = node.children.iter().find_map(|c| {
        if c.node_type != "attribute" {
            return None;
        }
        c.label
            .strip_prefix("include=")
            .or_else(|| c.label.strip_prefix("update=") )
            .map(|v| v.to_string())
    })?;
    Some(vec![
        "xml".to_string(),
        "msbuild".to_string(),
        node.label.clone(),
        include,
    ])
}

pub(crate) fn xml_msbuild_key(
    node: &SemanticNode,
    by_id: &HashMap<&str, &SemanticNode>,
) -> Option<Vec<String>> {
    xml_schema_key(node, by_id, &msbuild_coordinate_key)
}

pub(crate) fn xml_pom_key(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> Option<Vec<String>> {
    xml_schema_key(node, by_id, &pom_coordinate_key)
}

/// Shared key body for XML schema profiles: the element's own coordinate, or its nearest
/// keyed ancestor's coordinate + the tag path below it (text/attribute leaves get a
/// discriminating tail) so leaf values can only pair inside their owner.
pub(crate) fn xml_schema_key(
    node: &SemanticNode,
    by_id: &HashMap<&str, &SemanticNode>,
    coordinate_fn: &dyn Fn(&SemanticNode) -> Option<Vec<String>>,
) -> Option<Vec<String>> {
    if let Some(key) = coordinate_fn(node) {
        return Some(key);
    }
    // Descendants of a keyed element: owner coordinate + the path of tags below it, with a
    // trailing "text" segment for text nodes so leaf values can only pair inside their owner.
    // Attribute tails use only the attribute NAME (labels are `name=value` — keying the value
    // would break the very version-bump pairing this profile exists for).
    let mut lineage: Vec<String> = Vec::new();
    let mut current = node.id.clone();
    while let Some((parent_id, _)) = current.rsplit_once('.') {
        let Some(ancestor) = by_id.get(parent_id).copied() else {
            current = parent_id.to_string();
            continue;
        };
        if let Some(owner_key) = coordinate_fn(ancestor) {
            let mut key = owner_key;
            lineage.reverse();
            key.extend(lineage);
            match node.node_type.as_str() {
                "element" => key.push(node.label.clone()),
                "text" => key.push("text".to_string()),
                "attribute" => {
                    let name = node.label.split('=').next().unwrap_or(&node.label);
                    key.push(format!("attr:{name}"));
                }
                _ => return None,
            }
            return Some(key);
        }
        if ancestor.node_type == "element" {
            lineage.push(ancestor.label.clone());
        }
        current = parent_id.to_string();
    }
    None
}

// ── User-registered XML dialects (issue #86): the data-driven channel for the
// #63 descriptor registry. A dialect is a declarative coordinate spec — element
// tag -> key fields (child-element text, or `attr:NAME` attribute values) —
// interpreted through the same xml_schema_key body the bundled dialects use.
// Bundled dialects stay compiled (fast path + reference behavior) and OUTRANK
// user dialects; registration is process-level and match-predicated, so a
// dialect only ever applies to trees its namespace/root predicate claims.

use std::sync::{OnceLock, RwLock};

#[derive(Clone, serde::Deserialize)]
pub(crate) struct UserXmlDialect {
    pub(crate) language_id: String,
    #[serde(default)]
    pub(crate) root_element: Option<String>,
    #[serde(default)]
    pub(crate) namespace: Option<String>,
    pub(crate) keyed_elements: HashMap<String, Vec<String>>,
}

static USER_XML_DIALECTS: OnceLock<RwLock<Vec<UserXmlDialect>>> = OnceLock::new();

fn user_xml_dialects() -> &'static RwLock<Vec<UserXmlDialect>> {
    USER_XML_DIALECTS.get_or_init(|| RwLock::new(Vec::new()))
}

pub(crate) fn set_user_xml_dialects(dialects: Vec<UserXmlDialect>) -> usize {
    let count = dialects.len();
    if let Ok(mut guard) = user_xml_dialects().write() {
        *guard = dialects;
    }
    count
}

pub(crate) fn xml_tree_matches_user_dialect(dialect: &UserXmlDialect, tree: &SemanticNode) -> bool {
    if dialect.root_element.is_none() && dialect.namespace.is_none() {
        return false; // fail closed: a dialect must declare a match predicate
    }
    tree.children.iter().any(|top| {
        top.node_type == "element"
            && dialect
                .root_element
                .as_ref()
                .is_none_or(|root| &top.label == root)
            && dialect.namespace.as_ref().is_none_or(|ns| {
                top.children.iter().any(|c| {
                    c.node_type == "attribute"
                        && c.label
                            .split_once('=')
                            .is_some_and(|(name, value)| name.starts_with("xmlns") && value == ns)
                })
            })
    })
}

/// The declarative coordinate key: all fields must resolve, mirroring the
/// bundled dialects' Option semantics (a partially-keyed element is unkeyed).
pub(crate) fn user_dialect_coordinate_key(
    dialect: &UserXmlDialect,
    node: &SemanticNode,
) -> Option<Vec<String>> {
    if node.node_type != "element" {
        return None;
    }
    let fields = dialect.keyed_elements.get(node.label.as_str())?;
    let mut key = vec![
        "xml".to_string(),
        dialect.language_id.clone(),
        node.label.clone(),
    ];
    for field in fields {
        if let Some(attr) = field.strip_prefix("attr:") {
            let prefix = format!("{attr}=");
            let value = node.children.iter().find_map(|c| {
                if c.node_type != "attribute" {
                    return None;
                }
                c.label.strip_prefix(prefix.as_str()).map(str::to_string)
            })?;
            key.push(value);
        } else {
            key.push(pom_child_element_text(node, field)?);
        }
    }
    Some(key)
}

/// The first registered dialect whose predicate claims either tree.
pub(crate) fn matching_user_xml_dialect(
    old_tree: &SemanticNode,
    new_tree: &SemanticNode,
) -> Option<UserXmlDialect> {
    let guard = user_xml_dialects().read().ok()?;
    guard
        .iter()
        .find(|d| {
            xml_tree_matches_user_dialect(d, old_tree)
                || xml_tree_matches_user_dialect(d, new_tree)
        })
        .cloned()
}
