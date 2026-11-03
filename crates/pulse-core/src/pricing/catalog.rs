//! Strict parsers for the providers' public pricing tables. Fail closed on schema changes.
use super::Price;
use scraper::{Html, Selector};
use std::collections::BTreeMap;

pub const OPENAI_URL: &str = "https://developers.openai.com/api/docs/pricing.md";
pub const DEEPSEEK_URL: &str = "https://api-docs.deepseek.com/quick_start/pricing/";

fn money(value: &str) -> Result<Option<u64>, &'static str> {
    let value = value.trim();
    if value == "-" {
        return Ok(None);
    }
    let value = value.strip_prefix('$').ok_or("invalid currency")?;
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty()
        || !whole.bytes().all(|c| c.is_ascii_digit())
        || fraction.len() > 6
        || !fraction.bytes().all(|c| c.is_ascii_digit())
    {
        return Err("invalid price");
    }
    let whole = whole.parse::<u64>().map_err(|_| "invalid price")?;
    let fraction = format!("{fraction:0<6}")
        .parse::<u64>()
        .map_err(|_| "invalid price")?;
    let result = whole
        .checked_mul(1_000_000)
        .and_then(|n| n.checked_add(fraction))
        .ok_or("price overflow")?;
    if result > 1_000_000_000 {
        return Err("price out of range");
    }
    Ok(Some(result))
}
fn row(
    model: &str,
    tier: &str,
    date: &str,
    rates: &[&str],
    long: Option<bool>,
) -> Result<Option<Price>, &'static str> {
    let Some(input) = money(rates[0])? else {
        return Ok(None);
    };
    let Some(output) = money(rates[1])? else {
        return Ok(None);
    };
    Ok(Some(Price {
        version: format!("official-{date}-{model}-{tier}-{:?}", long),
        provider: "openai".into(),
        model: model.into(),
        service_tier: tier.into(),
        effective_from: format!("{date}T00:00:00Z"),
        effective_to: None,
        min_input: long.filter(|n| *n).map(|_| 272001),
        max_input: long.filter(|n| !*n).map(|_| 272000),
        input_microusd: input,
        output_microusd: output,
        cached_microusd: money(rates[2])?.unwrap_or(0),
        cached_supported: money(rates[2])?.is_some(),
        cache_write_microusd: rates.get(3).map(|r| money(r)).transpose()?.flatten(),
        cache_write_12h_microusd: rates.get(4).map(|r| money(r)).transpose()?.flatten(),
        source_url: OPENAI_URL.trim_end_matches(".md").into(),
        checked_at: date.into(),
    }))
}
pub fn openai(body: &str, date: &str) -> Result<Vec<Price>, &'static str> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| "invalid date")?;
    let mut tier = "";
    let mut million_tokens = false;
    let mut width = 0;
    let mut prices = BTreeMap::new();
    for line in body.lines().map(str::trim) {
        if line.starts_with("Prices per ") {
            million_tokens = line == "Prices per 1M tokens.";
            continue;
        }
        if line.starts_with('#') {
            width = 0;
            tier = match line {
                "### Standard pricing data" => "standard",
                "### Batch pricing data" | "#### Batch" => "batch",
                "#### Flex" => "flex",
                "### Fast pricing data" => "fast",
                "### Ultrafast pricing data" => "ultrafast",
                _ => "",
            };
            continue;
        }
        if !line.starts_with('|') || tier.is_empty() || !million_tokens {
            continue;
        }
        let cells = line
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect::<Vec<_>>();
        if cells.first() == Some(&"Model") {
            width = if cells == ["Model", "Input", "Output", "Cached input"] {
                4
            } else if cells
                == [
                    "Model",
                    "Short context input",
                    "Short context output",
                    "Short context cached input",
                    "Short context 30m write",
                    "Short context 12h write",
                    "Long context input",
                    "Long context output",
                    "Long context cached input",
                    "Long context 30m write",
                    "Long context 12h write",
                ]
            {
                11
            } else {
                0
            };
            if width == 11
                && !body.contains(
                    "Short context: ≤272K input tokens. Long context: >272K input tokens.",
                )
            {
                return Err("context threshold changed");
            }
            continue;
        }
        if width == 0 || cells.first().is_none_or(|m| m.starts_with('-')) {
            continue;
        }
        if cells.len() != width {
            return Err("pricing table changed");
        }
        let model = cells[0].split_whitespace().next().ok_or("invalid model")?;
        if model.len() > 100
            || !model
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b".-".contains(&c))
        {
            return Err("invalid model");
        }
        for (offset, long) in if width == 11 {
            vec![(1, Some(false)), (6, Some(true))]
        } else {
            vec![(1, None)]
        } {
            let count = if width == 11 { 5 } else { 3 };
            if let Some(p) = row(model, tier, date, &cells[offset..offset + count], long)? {
                let key = (
                    p.model.clone(),
                    p.service_tier.clone(),
                    p.min_input,
                    p.max_input,
                );
                if let Some(old) = prices.insert(key, p.clone())
                    && (old.input_microusd != p.input_microusd
                        || old.output_microusd != p.output_microusd
                        || old.cached_microusd != p.cached_microusd
                        || old.cache_write_microusd != p.cache_write_microusd
                        || old.cache_write_12h_microusd != p.cache_write_12h_microusd)
                {
                    return Err("conflicting prices");
                }
            }
        }
    }
    if !prices.values().any(|p| {
        p.model == "gpt-6.1-sol" && p.service_tier == "standard" && p.max_input == Some(272000)
    }) || prices.len() < 8
        || prices.len() > 4096
    {
        return Err("incomplete OpenAI catalog");
    }
    Ok(prices.into_values().collect())
}
pub fn deepseek(body: &str, date: &str) -> Result<Vec<Price>, &'static str> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| "invalid date")?;
    let html = Html::parse_document(body);
    let selector = |s| Selector::parse(s).expect("static selector");
    let table = html
        .select(&selector("table"))
        .next()
        .ok_or("missing DeepSeek table")?;
    let rows = table
        .select(&selector("tr"))
        .map(|tr| {
            tr.select(&selector("td,th"))
                .map(|cell| {
                    cell.text()
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let models = rows
        .first()
        .ok_or("missing models")?
        .iter()
        .skip(1)
        .map(|name| name.split('(').next().unwrap().trim())
        .collect::<Vec<_>>();
    if models.is_empty()
        || models.len() > 16
        || !models.contains(&"deepseek-flash")
        || models.iter().any(|model| {
            !model.starts_with("deepseek-")
                || model.len() > 100
                || !model
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b".-".contains(&c))
        })
    {
        return Err("DeepSeek models changed");
    }
    let mut rates = Vec::new();
    for cells in &rows {
        let values = cells
            .iter()
            .filter(|c| c.starts_with('$'))
            .map(|c| money(c).and_then(|n| n.ok_or("missing price")))
            .collect::<Result<Vec<_>, _>>()?;
        if !values.is_empty() {
            if values.len() != models.len() || !cells.iter().any(|c| c == "PEAK" || c == "OFF-PEAK")
            {
                return Err("DeepSeek prices changed");
            }
            let label = cells.join(" ");
            let expected = [
                "CACHE HIT",
                "PEAK",
                "CACHE MISS",
                "PEAK",
                "1M OUTPUT TOKENS",
                "PEAK",
            ];
            if rates.len() >= expected.len()
                || !label.contains(expected[rates.len()])
                || (rates.len() % 2 == 0 && !cells.iter().any(|c| c == "OFF-PEAK"))
                || (rates.len() % 2 == 1 && cells.iter().any(|c| c == "OFF-PEAK"))
            {
                return Err("DeepSeek rate order changed");
            }
            rates.push(values);
        }
    }
    if rates.len() != 6
        || !rows.iter().flatten().any(|c| c.contains("CACHE HIT"))
        || !rows.iter().flatten().any(|c| c.contains("CACHE MISS"))
    {
        return Err("incomplete DeepSeek catalog");
    }
    if (0..models.len()).any(|i| (0..3).any(|group| rates[group * 2][i] > rates[group * 2 + 1][i]))
    {
        return Err("invalid peak prices");
    }
    let mut prices = Vec::new();
    for (i, model) in models.iter().enumerate() {
        for (offset, tier) in [(0, "off_peak"), (1, "peak")] {
            if rates[offset][i] > rates[2 + offset][i] {
                return Err("invalid cache rate");
            }
            let mut p = row(model, tier, date, &["$0", "$0", "$0"], None)?.unwrap();
            p.provider = "deepseek".into();
            p.source_url = DEEPSEEK_URL.into();
            p.input_microusd = rates[2 + offset][i];
            p.cached_microusd = rates[offset][i];
            p.output_microusd = rates[4 + offset][i];
            prices.push(p);
        }
    }
    Ok(prices)
}
