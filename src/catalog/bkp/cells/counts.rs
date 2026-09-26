//! How many cells hold each categorical value, and — crossed against the
//! column the points are colored by — in which colors.

use super::*;

/// How many cells hold each value of one categorical column, and — when
/// `against` names a second column — how many hold each pairing of the two,
/// among the cells `filters` admit: every one of its groups, and any
/// condition in a group.
///
/// Crossing two columns is one query rather than one per value: the API
/// groups by as many fields as it is given and returns only the pairings that
/// occur, so a taxonomy crossed with its own coloring costs a row per cluster
/// rather than a row per cluster per color.
pub(super) async fn counts(
    endpoint: &str,
    filter: &Value,
    field: &str,
    codes: &HashMap<String, u16>,
    against: Option<(&str, &HashMap<String, u16>)>,
    filters: &Value,
) -> Result<(Vec<(u16, u64)>, Vec<(u16, u16, u64)>), String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Data {
        cell_counts: Vec<Counted>,
    }

    let fields: Vec<&str> = [field]
        .into_iter()
        .chain(against.map(|(id, _)| id))
        .collect();
    let variables = json!({ "filter": filter, "fields": fields, "filters": filters });
    let data: Data = ask(endpoint, COUNTS, variables).await?;
    Ok(tally(&data.cell_counts, field, codes, against))
}

/// Put each counted group back on its codes: the column's own count, summed
/// across whatever it was crossed with, and the crossing itself.
///
/// Each group names the column every one of its values came from, so a
/// crossed pair is read by name rather than by trusting the order it comes
/// back in. A group holding a label neither column knows is dropped.
pub(super) fn tally<'a>(
    groups: &'a [Counted],
    field: &str,
    codes: &HashMap<String, u16>,
    against: Option<(&str, &HashMap<String, u16>)>,
) -> (Vec<(u16, u64)>, Vec<(u16, u16, u64)>) {
    let held = |counted: &'a Counted, column: &str| -> Option<&'a str> {
        counted
            .properties
            .iter()
            .find(|tuple| tuple.property.as_deref() == Some(column))
            // One group has nothing to tell apart, and the range counts name
            // no column at all.
            .or_else(|| counted.properties.first().filter(|_| against.is_none()))
            .and_then(|tuple| tuple.value.as_deref())
    };

    let mut totals: HashMap<u16, u64> = HashMap::new();
    let mut mixes = Vec::new();
    for counted in groups {
        let Some(code) = held(counted, field).and_then(|label| codes.get(label).copied()) else {
            continue;
        };
        let count = counted.count as u64;
        *totals.entry(code).or_default() += count;
        if let Some((column, against)) = against
            && let Some(color) = held(counted, column).and_then(|label| against.get(label).copied())
        {
            mixes.push((code, color, count));
        }
    }
    let mut values: Vec<(u16, u64)> = totals.into_iter().collect();
    values.sort_unstable();
    (values, mixes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two groups of a real crossed answer: the API returns one row per
    /// pairing that occurs, each value named by the column it came from.
    const CROSSED_TEXT: &str = r#"[
      { "count": 2144, "properties": [
          { "property": "NEUROTRANSMITTER", "value": "Chol" },
          { "property": "CLASS", "value": "17 MH-LH Glut" } ] },
      { "count": 1778, "properties": [
          { "property": "NEUROTRANSMITTER", "value": "Chol" },
          { "property": "CLASS", "value": "08 CNU-MGE GABA" } ] },
      { "count": 6121, "properties": [
          { "property": "NEUROTRANSMITTER", "value": "Dopa" },
          { "property": "CLASS", "value": "05 OB-IMN GABA" } ] },
      { "count": 12, "properties": [
          { "property": "NEUROTRANSMITTER", "value": "Chol" },
          { "property": "CLASS", "value": "a class nobody described" } ] }
    ]"#;

    fn crossed() -> (Vec<(u16, u64)>, Vec<(u16, u16, u64)>) {
        let groups: Vec<Counted> = serde_json::from_str(CROSSED_TEXT).unwrap();
        let codes = HashMap::from([("Chol".to_string(), 7), ("Dopa".to_string(), 9)]);
        let colors = HashMap::from([
            ("17 MH-LH Glut".to_string(), 17),
            ("08 CNU-MGE GABA".to_string(), 8),
            ("05 OB-IMN GABA".to_string(), 5),
        ]);
        let (values, mixes) = tally(
            &groups,
            "NEUROTRANSMITTER",
            &codes,
            Some(("CLASS", &colors)),
        );
        // Owned, so the test's maps do not have to outlive the answer.
        (values, mixes)
    }

    #[test]
    fn a_crossed_count_adds_up_to_the_plain_one() {
        let (values, _) = crossed();
        // Chol's three groups, the undescribed class among them, and Dopa's one.
        assert_eq!(values, [(7, 2144 + 1778 + 12), (9, 6121)]);
    }

    #[test]
    fn a_crossed_count_keeps_the_pairings_it_can_name() {
        let (_, mixes) = crossed();
        assert_eq!(mixes, [(7, 17, 2144), (7, 8, 1778), (9, 5, 6121)]);
    }

    #[test]
    fn an_uncrossed_count_reads_the_one_value_a_group_holds() {
        let groups: Vec<Counted> =
            serde_json::from_str(r#"[{ "count": 40, "properties": [{ "value": "Braak 0" }] }]"#)
                .unwrap();
        let codes = HashMap::from([("Braak 0".to_string(), 5)]);
        let (values, mixes) = tally(&groups, "BRAAK", &codes, None);
        assert_eq!(values, [(5, 40)]);
        assert!(mixes.is_empty());
    }
}
