//! What the registry holds of each kind of record, and how a record of each
//! is read into a row.
//!
//! Columns and filters are written out rather than discovered: the registry's
//! records are typed, and every specimen, process or data asset has the same
//! fields as the next. What varies is in each record's `data`, which differs
//! by schema and can run to thousands of file paths, so it is never asked
//! for.

use serde_json::{Map, Value, json};

use crate::source::table::{SortKey, TableFilterTerm};

/// The three kinds of record the Pre-Public Data Catalog lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Specimens,
    Processes,
    DataAssets,
}

/// The column every kind names its records under.
pub(super) const NAME: &str = "Name";

/// The column every kind opens sorted by, newest first.
pub(super) const CREATED: &str = "Created";

/// The column holding a record's own id, which is what its links are asked
/// for by.
pub(super) const ID: &str = "Id";

/// How a column's values read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    Text,
    /// A time, to the minute.
    Time,
    /// An id elsewhere, as the system that holds it and the id there.
    Identifier,
    /// Yes or no.
    Flag,
}

pub(super) struct Column {
    pub(super) name: &'static str,
    /// Where it is read from in a record, a step to a dot. A list along the
    /// way is read whole, its values joined.
    pub(super) path: &'static str,
    pub(super) shape: Shape,
    /// What the registry orders by for it, in the same steps; none if it
    /// cannot order by it.
    pub(super) sort: Option<&'static str>,
    pub(super) hidden: bool,
}

const fn column(name: &'static str, path: &'static str, sort: Option<&'static str>) -> Column {
    Column {
        name,
        path,
        shape: Shape::Text,
        sort,
        hidden: false,
    }
}

const fn hidden(column: Column) -> Column {
    Column {
        hidden: true,
        ..column
    }
}

const fn shaped(column: Column, shape: Shape) -> Column {
    Column { shape, ..column }
}

/// Where the values a filter offers come from.
pub(super) enum Vocabulary {
    /// Written into the schema as an enum.
    Fixed(&'static [&'static str]),
    /// Listed by the registry: the root field, and the field of each node
    /// naming a value.
    Listed(&'static str, &'static str),
}

/// A filter: what it is called, where the registry narrows by it, and what
/// it offers.
pub(super) struct Facet {
    /// The steps the registry's `where` nests through to it, a step to a dot.
    pub(super) path: &'static str,
    pub(super) name: &'static str,
    pub(super) values: Vocabulary,
}

const SPECIMENS: &[Column] = &[
    column(NAME, "name", Some("name")),
    column("Type", "specimenType.label", Some("specimenType.label")),
    column("Species", "subjects.species.name", None),
    column("Subject", "subjects.name", None),
    column("State", "state", Some("state")),
    shaped(
        column("External ids", "externalIdentifiers", None),
        Shape::Identifier,
    ),
    hidden(column("Projects", "projects.externalId", None)),
    shaped(column(CREATED, "createdAt", Some("createdAt")), Shape::Time),
    hidden(column(ID, "id", Some("id"))),
];

const PROCESSES: &[Column] = &[
    column(NAME, "name", Some("name")),
    column("Kind", "schemaType.name", Some("schemaType.name")),
    hidden(column("Version", "schemaType.version", None)),
    // Every process on the stage registry was of type Process on 2026-09-27,
    // so it is neither shown at first nor offered as a filter.
    hidden(column("Type", "type.name", Some("type.name"))),
    column("State", "state", Some("state.name")),
    column("Instruments", "instruments.name", None),
    hidden(column("Projects", "projects.externalId", None)),
    column("Comments", "comments", Some("comments")),
    shaped(column(CREATED, "createdAt", Some("createdAt")), Shape::Time),
    hidden(column(ID, "id", Some("id"))),
];

const DATA_ASSETS: &[Column] = &[
    column(NAME, "name", Some("name")),
    column("Type", "type", Some("type.name")),
    column("Status", "status", Some("status")),
    column("Address", "instances.downloadUrl", None),
    shaped(
        column(
            "Controlled access",
            "requiresControlledAccess",
            Some("requiresControlledAccess"),
        ),
        Shape::Flag,
    ),
    hidden(column("Use", "use", Some("use.name"))),
    hidden(column("DOI", "doi", Some("doi"))),
    hidden(column("Tags", "tags", None)),
    shaped(column(CREATED, "createdAt", Some("createdAt")), Shape::Time),
    hidden(column(ID, "id", Some("id"))),
];

const SPECIMEN_FACETS: &[Facet] = &[
    Facet {
        path: "specimenType.label",
        name: "Type",
        values: Vocabulary::Listed("specimenTypes", "label"),
    },
    Facet {
        path: "subjects.some.species.name",
        name: "Species",
        values: Vocabulary::Listed("species", "name"),
    },
    Facet {
        path: "state",
        name: "State",
        values: Vocabulary::Fixed(&["PENDING", "ARCHIVED"]),
    },
];

const PROCESS_FACETS: &[Facet] = &[
    Facet {
        path: "schemaType.name",
        name: "Kind",
        values: Vocabulary::Listed("processSchemaTypes", "name"),
    },
    Facet {
        path: "state.name",
        name: "State",
        values: Vocabulary::Listed("processStates", "name"),
    },
];

const DATA_ASSET_FACETS: &[Facet] = &[
    Facet {
        path: "type.name",
        name: "Type",
        values: Vocabulary::Listed("dataAssetTypes", "name"),
    },
    Facet {
        path: "status",
        name: "Status",
        values: Vocabulary::Fixed(&[
            "PENDING_REVIEW",
            "PUBLISHED",
            "QC_PASSED",
            "QC_FAILED",
            "RETRACTED",
            "ARCHIVED",
        ]),
    },
];

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Specimens, Kind::Processes, Kind::DataAssets];

    /// What the registry's query for them is called, which is also what an
    /// address names them by.
    pub fn root(self) -> &'static str {
        match self {
            Kind::Specimens => "specimens",
            Kind::Processes => "processes",
            Kind::DataAssets => "dataAssets",
        }
    }

    pub fn from_root(root: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.root() == root)
    }

    pub fn title(self) -> &'static str {
        match self {
            Kind::Specimens => "Specimens",
            Kind::Processes => "Processes",
            Kind::DataAssets => "Data assets",
        }
    }

    pub(super) fn filter_type(self) -> &'static str {
        match self {
            Kind::Specimens => "SpecimenFilterInput",
            Kind::Processes => "ProcessFilterInput",
            Kind::DataAssets => "DataAssetFilterInput",
        }
    }

    pub(super) fn sort_type(self) -> &'static str {
        match self {
            Kind::Specimens => "SpecimenSortInput",
            Kind::Processes => "ProcessSortInput",
            Kind::DataAssets => "DataAssetSortInput",
        }
    }

    pub(super) fn columns(self) -> &'static [Column] {
        match self {
            Kind::Specimens => SPECIMENS,
            Kind::Processes => PROCESSES,
            Kind::DataAssets => DATA_ASSETS,
        }
    }

    pub(super) fn facets(self) -> &'static [Facet] {
        match self {
            Kind::Specimens => SPECIMEN_FACETS,
            Kind::Processes => PROCESS_FACETS,
            Kind::DataAssets => DATA_ASSET_FACETS,
        }
    }

    /// What is asked of each record for its row.
    pub(super) fn selection(self) -> &'static str {
        match self {
            Kind::Specimens => {
                "id name state createdAt specimenType { label } \
                 subjects { name species { name } } \
                 externalIdentifiers { externalId source { name } } \
                 projects { externalId }"
            }
            Kind::Processes => {
                "id name state createdAt comments type { name } \
                 schemaType { name version } instruments { name } \
                 projects { externalId }"
            }
            Kind::DataAssets => {
                "id name type status use createdAt doi requiresControlledAccess \
                 tags instances { downloadUrl }"
            }
        }
    }

    pub(super) fn headers(self) -> Vec<String> {
        self.columns()
            .iter()
            .map(|column| column.name.to_string())
            .collect()
    }

    pub(super) fn hidden(self) -> impl Iterator<Item = bool> {
        self.columns().iter().map(|column| column.hidden)
    }

    /// A page of records as rows, in the columns' order.
    pub(super) fn rows(self, nodes: &[Value]) -> Vec<Vec<String>> {
        nodes
            .iter()
            .map(|node| {
                self.columns()
                    .iter()
                    .map(|column| read(node, column.path, column.shape))
                    .collect()
            })
            .collect()
    }

    /// The registry's `order` for a table sorted by `keys`. A column it
    /// cannot order by is passed over.
    pub(super) fn order(self, keys: &[SortKey]) -> Value {
        let order: Vec<Value> = keys
            .iter()
            .filter_map(|key| {
                let column = self.columns().iter().find(|it| it.name == key.column)?;
                let direction = if key.descending { "DESC" } else { "ASC" };
                Some(nest(column.sort?, json!(direction)))
            })
            .collect();
        if order.is_empty() {
            Value::Null
        } else {
            Value::Array(order)
        }
    }
}

/// `leaf` under each of `path`'s steps in turn.
pub(super) fn nest(path: &str, leaf: Value) -> Value {
    path.rsplit('.').fold(leaf, |inner, step| {
        let mut outer = Map::new();
        outer.insert(step.to_string(), inner);
        Value::Object(outer)
    })
}

/// Every one of `wheres` at once, dropping any that ask nothing.
pub(super) fn all_of(wheres: Vec<Value>) -> Value {
    let mut wheres: Vec<Value> = wheres.into_iter().filter(|it| !it.is_null()).collect();
    match wheres.len() {
        0 => Value::Null,
        1 => wheres.remove(0),
        _ => json!({ "and": wheres }),
    }
}

/// The registry's `where` for what has been asked of a table.
///
/// Values ticked in one column widen, so they are asked for together with
/// `in`; columns narrow each other, so they are asked for together with
/// `and`.
pub(super) fn where_of(terms: &[TableFilterTerm]) -> Value {
    let mut fields: Vec<(&str, Vec<&str>)> = Vec::new();
    for term in terms {
        let TableFilterTerm::Is { field, value } = term else {
            continue;
        };
        match fields.iter_mut().find(|(it, _)| it == field) {
            Some((_, values)) => values.push(value),
            None => fields.push((field, vec![value])),
        }
    }
    all_of(
        fields
            .into_iter()
            .map(|(field, values)| match values.as_slice() {
                [value] => nest(field, json!({ "eq": value })),
                _ => nest(field, json!({ "in": values })),
            })
            .collect(),
    )
}

/// Everything asked of a table: the values ticked, and the text searched
/// for.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Asked {
    pub(super) terms: Vec<TableFilterTerm>,
    pub(super) search: Option<String>,
}

/// What a search is matched against: every kind's records are named, and
/// nothing else about them reads as words.
pub(super) const SEARCHED: &str = "name";

impl Asked {
    /// The registry's `where` for it. A search matches names holding the
    /// text anywhere, in any case, which the registry answered in under a
    /// second even across 17.8 million data assets.
    pub(super) fn where_value(&self) -> Value {
        all_of(vec![
            where_of(&self.terms),
            self.search.as_ref().map_or(Value::Null, |text| {
                nest(SEARCHED, json!({ "containsInsensitive": text }))
            }),
        ])
    }
}

/// What a record holds at `path`, as it reads in a cell.
pub(super) fn read(node: &Value, path: &str, shape: Shape) -> String {
    let steps: Vec<&str> = path.split('.').collect();
    let mut found = Vec::new();
    leaves(node, &steps, &mut found);
    let mut texts: Vec<String> = Vec::new();
    for text in found.into_iter().filter_map(|value| shape_of(value, shape)) {
        if !text.is_empty() && !texts.contains(&text) {
            texts.push(text);
        }
    }
    texts.join(", ")
}

fn leaves<'a>(value: &'a Value, steps: &[&str], found: &mut Vec<&'a Value>) {
    match value {
        Value::Null => {}
        Value::Array(items) => {
            for item in items {
                leaves(item, steps, found);
            }
        }
        _ => match steps.split_first() {
            None => found.push(value),
            Some((step, rest)) => {
                if let Some(next) = value.get(step) {
                    leaves(next, rest, found);
                }
            }
        },
    }
}

fn shape_of(value: &Value, shape: Shape) -> Option<String> {
    match (shape, value) {
        (Shape::Identifier, _) => {
            let id = value.get("externalId")?.as_str()?;
            match value.pointer("/source/name").and_then(Value::as_str) {
                Some(source) => Some(format!("{source} {id}")),
                None => Some(id.to_string()),
            }
        }
        (Shape::Flag, Value::Bool(flag)) => Some(if *flag { "Yes" } else { "No" }.to_string()),
        (Shape::Time, Value::String(text)) => Some(time_of(text)),
        (_, Value::String(text)) => Some(text.trim().to_string()),
        (_, Value::Number(number)) => Some(number.to_string()),
        (_, Value::Bool(flag)) => Some(flag.to_string()),
        _ => None,
    }
}

/// An ISO 8601 time, to the minute, as a table reads it.
pub(super) fn time_of(text: &str) -> String {
    match text.split_once('T') {
        Some((date, time)) => format!("{date} {}", time.get(..5).unwrap_or(time)),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_column_is_asked_for() {
        for kind in Kind::ALL {
            let selection = kind.selection();
            for column in kind.columns() {
                let first = column.path.split('.').next().unwrap();
                assert!(
                    selection
                        .split(|c: char| !c.is_alphanumeric())
                        .any(|word| word == first),
                    "{kind:?} does not ask for {first}"
                );
            }
            assert!(kind.columns().iter().any(|it| it.name == ID));
            assert!(kind.columns().iter().any(|it| it.name == CREATED));
            assert_eq!(Kind::from_root(kind.root()), Some(kind));
        }
    }

    #[test]
    fn a_record_reads_into_a_row() {
        // As the stage registry answered, on 2026-09-27.
        let node: Value = serde_json::from_str(
            r#"{"id": "00024c27", "name": "C57BL6J-735467.282", "state": "PENDING",
            "createdAt": "2024-05-13T20:25:42.000Z",
            "specimenType": {"label": "brain specimen section"},
            "subjects": [{"name": "1344855095", "species": {"name": "Mus musculus"}}],
            "externalIdentifiers": [{"externalId": "1344858799", "source": {"name": "LIMS2"}}],
            "projects": []}"#,
        )
        .unwrap();
        let row = &Kind::Specimens.rows(&[node])[0];
        let cell = |name: &str| {
            let at = Kind::Specimens.headers().iter().position(|it| it == name);
            row[at.unwrap()].clone()
        };
        assert_eq!(cell("Type"), "brain specimen section");
        assert_eq!(cell("Species"), "Mus musculus");
        assert_eq!(cell("External ids"), "LIMS2 1344858799");
        assert_eq!(cell("Projects"), "");
        assert_eq!(cell(CREATED), "2024-05-13 20:25");
    }

    #[test]
    fn a_list_reads_as_its_values_once_each() {
        let node = json!({
            "instances": [{"downloadUrl": "s3://a/x.zarr"}, {"downloadUrl": "s3://b/x.zarr"}],
            "tags": ["a", "b", "a"],
            "requiresControlledAccess": false,
        });
        assert_eq!(
            read(&node, "instances.downloadUrl", Shape::Text),
            "s3://a/x.zarr, s3://b/x.zarr"
        );
        assert_eq!(read(&node, "tags", Shape::Text), "a, b");
        assert_eq!(read(&node, "requiresControlledAccess", Shape::Flag), "No");
        assert_eq!(read(&node, "missing.field", Shape::Text), "");
    }

    #[test]
    fn a_sort_nests_as_the_registry_orders() {
        let keys = [
            SortKey {
                column: "Type".into(),
                descending: false,
            },
            SortKey {
                column: CREATED.into(),
                descending: true,
            },
            // Nothing to order a list of species by.
            SortKey {
                column: "Species".into(),
                descending: false,
            },
        ];
        assert_eq!(
            Kind::Specimens.order(&keys),
            json!([{"specimenType": {"label": "ASC"}}, {"createdAt": "DESC"}])
        );
        assert_eq!(Kind::Processes.order(&[]), Value::Null);
    }

    #[test]
    fn ticks_in_one_column_widen_and_columns_narrow() {
        let is = |field: &str, value: &str| TableFilterTerm::Is {
            field: field.into(),
            value: value.into(),
        };
        assert_eq!(where_of(&[]), Value::Null);
        let searched = Asked {
            terms: vec![is("status", "PUBLISHED")],
            search: Some("neuroglancer".into()),
        };
        assert_eq!(
            searched.where_value(),
            json!({"and": [
                {"status": {"eq": "PUBLISHED"}},
                {"name": {"containsInsensitive": "neuroglancer"}},
            ]})
        );
        assert_eq!(
            where_of(&[is("status", "PUBLISHED")]),
            json!({"status": {"eq": "PUBLISHED"}})
        );
        assert_eq!(
            where_of(&[
                is("subjects.some.species.name", "Homo sapiens"),
                is("state", "PENDING"),
                is("subjects.some.species.name", "Mus musculus"),
            ]),
            json!({"and": [
                {"subjects": {"some": {"species": {"name": {"in": ["Homo sapiens", "Mus musculus"]}}}}},
                {"state": {"eq": "PENDING"}},
            ]})
        );
    }
}
