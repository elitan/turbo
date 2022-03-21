use std::{collections::{HashMap, BTreeMap}};

use anyhow::{anyhow, bail, Result, Context};
use json::{JsonValue};

use super::{options::ConditionValue, prefix_tree::{WildcardReplacable, PrefixTree, PrefixTreeIterator}};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ExportsValue {
    Alternatives(Vec<ExportsValue>),
    Conditional(Vec<(String, ExportsValue)>),
    Result(String),
    Excluded,
}

impl WildcardReplacable for ExportsValue {
    fn replace_wildcard(&self, value: &str) -> Result<Self> {
        Ok(match self {
            ExportsValue::Alternatives(list) => ExportsValue::Alternatives(
                list.iter()
                    .map(|v| v.replace_wildcard(value))
                    .collect::<Result<Vec<_>>>()?,
            ),
            ExportsValue::Conditional(list) => ExportsValue::Conditional(
                list.iter()
                    .map(|(c, v)| Ok((c.clone(), v.replace_wildcard(value)?)))
                    .collect::<Result<Vec<_>>>()?,
            ),
            ExportsValue::Result(v) => {
                if !v.contains("*") {
                    bail!("exports field value need to contain a wildcard (*) when the key contains one");
                }
                ExportsValue::Result(v.replace("*", value))
            }
            ExportsValue::Excluded => ExportsValue::Excluded,
        })
    }

    fn append_to_folder(&self, value: &str) -> Result<Self> {
        Ok(match self {
            ExportsValue::Alternatives(list) => ExportsValue::Alternatives(
                list.iter()
                    .map(|v| v.append_to_folder(value))
                    .collect::<Result<Vec<_>>>()?,
            ),
            ExportsValue::Conditional(list) => ExportsValue::Conditional(
                list.iter()
                    .map(|(c, v)| Ok((c.clone(), v.append_to_folder(value)?)))
                    .collect::<Result<Vec<_>>>()?,
            ),
            ExportsValue::Result(v) => {
                if !v.ends_with("/") {
                    bail!("exports field value need ends with '/' when the key ends with it");
                }
                ExportsValue::Result(v.to_string() + value)
            }
            ExportsValue::Excluded => ExportsValue::Excluded,
        })
    }
}

impl ExportsValue {
    pub fn add_results<'a>(&'a self, conditions: &BTreeMap<String, ConditionValue>, unspecified_condition: &ConditionValue, condition_overrides: &mut HashMap<&'a str, ConditionValue>, target: &mut Vec<&'a str>) -> bool {
        match self {
            ExportsValue::Alternatives(list) => {
                for value in list {
                    if value.add_results(conditions, unspecified_condition, condition_overrides, target) {
                        return true;
                    }
                }
                return false;
            },
            ExportsValue::Conditional(list) => {
                for (condition, value) in list {
                    let condition_value = condition_overrides.get(condition.as_str())
                        .or_else(|| conditions.get(condition))
                        .unwrap_or_else(|| unspecified_condition);
                    match condition_value {
                        ConditionValue::Set => {
                            if value.add_results(conditions, unspecified_condition, condition_overrides, target) {
                                return true;
                            }
                        },
                        ConditionValue::Unset => {},
                        ConditionValue::Unknown => {
                            condition_overrides.insert(condition, ConditionValue::Set);
                            if value.add_results(conditions, unspecified_condition, condition_overrides, target) {
                                condition_overrides.insert(condition, ConditionValue::Unset);
                            } else {
                                condition_overrides.remove(condition.as_str());
                            }
                        },
                    }
                }
                return false;
            },
            ExportsValue::Result(r) => {
                target.push(r);
                return true;
            },
            ExportsValue::Excluded => {
                return true;
            },
        }
    }
}

impl TryFrom<&JsonValue> for ExportsValue {
    type Error = anyhow::Error;
    fn try_from(value: &JsonValue) -> Result<Self> {
        match value {
            JsonValue::Null => Ok(ExportsValue::Excluded),
            JsonValue::Short(s) => Ok(ExportsValue::Result(s.to_string())),
            JsonValue::String(s) => Ok(ExportsValue::Result(s.to_string())),
            JsonValue::Number(_) => Err(anyhow!(
                "numeric values are invalid in exports/imports field"
            )),
            JsonValue::Boolean(_) => Err(anyhow!(
                "boolean values are invalid in exports/imports field"
            )),
            JsonValue::Object(o) => Ok(ExportsValue::Conditional(
                o.iter()
                    .map(|(key, value)| {
                        if key.starts_with(".") || key.starts_with("#") {
                            bail!("invalid key \"{}\" in an conditions object (Did you want to place this request on higher level?)", key);
                        } 
                        Ok((key.to_string(), value.try_into()?))})
                    .collect::<Result<Vec<_>>>()?,
            )),
            JsonValue::Array(a) => Ok(ExportsValue::Alternatives(a.iter().map(|value| Ok(value.try_into()?)).collect::<Result<Vec<_>>>()?)),
        }
    }
}

#[derive(PartialEq, Eq)]
pub struct ExportsField(PrefixTree<ExportsValue>);

#[derive(PartialEq, Eq)]
pub struct ImportsField(PrefixTree<ExportsValue>);

impl TryFrom<&JsonValue> for ExportsField {
    type Error = anyhow::Error;

    fn try_from(value: &JsonValue) -> Result<Self> {
        Ok(Self(PrefixTree::from_json(value, |request| request == "." || request.starts_with("./"), Some(".")).with_context(|| anyhow!("failed to parse 'exports' field value"))?))
    }
}

impl TryFrom<&JsonValue> for ImportsField {
    type Error = anyhow::Error;

    fn try_from(value: &JsonValue) -> Result<Self> {
        Ok(Self(PrefixTree::from_json(value, |request| request == "." || request.starts_with("./"), Some(".")).with_context(|| anyhow!("failed to parse 'exports' field value"))?))
    }
}

impl ExportsField {
    pub fn lookup<'a>(&'a self, request: &'a str) -> PrefixTreeIterator<'a, ExportsValue> {
        self.0.lookup(request)
    }
}

impl ImportsField {
    pub fn lookup<'a>(&'a self, request: &'a str) -> PrefixTreeIterator<'a, ExportsValue> {
        self.0.lookup(request)
    }
}