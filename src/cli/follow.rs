use std::collections::{BTreeMap, HashMap};
use std::fs;

use anyhow::Result;

use crate::cli::args::FollowArgs;
use crate::models::{
    CallRef, FollowCallSite, FollowDirection, FollowEdge, FollowResult, FollowSymbolRef,
    FollowTarget, SearchResult, SymbolKind, FOLLOW_RESULT_VERSION,
};

/// Build a `FollowResult` from a symbol-mode `SearchResult` and a
/// requested follow direction.
pub fn build_follow_result(
    result: &SearchResult,
    direction: FollowDirection,
) -> FollowResult {
    let mut targets = Vec::new();

    for symbol in &result.symbols {
        let mut target = FollowTarget {
            symbol: symbol.clone(),
            callers: Vec::new(),
            callees: Vec::new(),
        };

        if matches!(direction, FollowDirection::Callers | FollowDirection::Both) {
            target.callers = group_call_edges(&symbol.called_by);
        }

        if matches!(direction, FollowDirection::Callees | FollowDirection::Both) {
            target.callees = group_call_edges(&symbol.calls);
        }

        targets.push(target);
    }

    FollowResult {
        version: FOLLOW_RESULT_VERSION.to_string(),
        direction,
        query: result.query.clone(),
        targets,
    }
}

fn group_call_edges(edges: &[CallRef]) -> Vec<FollowEdge> {
    #[derive(Default)]
    struct TempGroup {
        kind: Option<SymbolKind>,
        call_sites: Vec<FollowCallSite>,
    }

    let mut grouped: BTreeMap<(String, std::path::PathBuf), TempGroup> = BTreeMap::new();

    for edge in edges {
        let Some(line) = edge.line else {
            continue;
        };

        let key = (edge.name.clone(), edge.file.clone());
        let entry = grouped.entry(key).or_default();

        if entry.kind.is_none() {
            entry.kind = edge.kind;
        }

        entry.call_sites.push(FollowCallSite {
            file: edge.file.clone(),
            line,
            column: None,
        });
    }

    let mut result = Vec::new();

    for ((name, file), mut group) in grouped {
        if group.call_sites.is_empty() {
            continue;
        }

        // Ensure call sites are ordered by line number for stable output.
        group
            .call_sites
            .sort_by_key(|site| (site.line, site.column.unwrap_or(0)));

        let symbol = FollowSymbolRef {
            name,
            kind: group.kind,
            file: file.clone(),
        };

        result.push(FollowEdge {
            symbol,
            call_sites: group.call_sites,
        });
    }

    result
}

/// Render a `FollowResult` in human-readable text form.
///
/// Layout:
/// - Per target symbol: header line.
/// - Per caller/callee group: header + context window(s) around call sites.
pub fn print_follow_text(result: &FollowResult, args: &FollowArgs) -> Result<()> {
    let context = args.context.unwrap_or(0) as u32;
    let max_lines_per_block = args.max_lines.unwrap_or(usize::MAX);

    if result.targets.is_empty() {
        return Ok(());
    }

    let mut first_target = true;
    let mut file_cache: HashMap<String, Vec<String>> = HashMap::new();

    for target in &result.targets {
        if !first_target {
            println!();
        }
        first_target = false;

        let file = target.symbol.file.display().to_string();
        let kind = format!("{:?}", target.symbol.kind).to_lowercase();
        let line = target.symbol.range.start_line;
        println!("Target: {} ({kind})  [{file}:{line}]", target.symbol.name);

        if max_lines_per_block == 0 {
            continue;
        }

        let want_callers = matches!(
            result.direction,
            FollowDirection::Callers | FollowDirection::Both
        );
        let want_callees = matches!(
            result.direction,
            FollowDirection::Callees | FollowDirection::Both
        );

        if want_callers && !target.callers.is_empty() {
            println!();
            print_edge_groups(
                "Caller",
                &target.callers,
                &target.symbol.name,
                context,
                max_lines_per_block,
                &mut file_cache,
                /*highlight_with_target_name=*/ true,
            );
        }

        if want_callees && !target.callees.is_empty() {
            println!();
            print_edge_groups(
                "Callee",
                &target.callees,
                &target.symbol.name,
                context,
                max_lines_per_block,
                &mut file_cache,
                /*highlight_with_target_name=*/ false,
            );
        }
    }

    Ok(())
}

fn load_file_lines<'a>(
    path: &str,
    cache: &'a mut HashMap<String, Vec<String>>,
) -> Option<&'a [String]> {
    if !cache.contains_key(path) {
        let contents = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => {
                eprintln!(
                    "warning: failed to read source file for follow context: {}",
                    path
                );
                return None;
            }
        };
        let lines: Vec<String> = contents.lines().map(|s| s.to_string()).collect();
        cache.insert(path.to_string(), lines);
    }

    cache.get(path).map(|v| v.as_slice())
}

fn print_edge_groups(
    label: &str,
    groups: &[FollowEdge],
    target_name: &str,
    context: u32,
    max_lines_per_block: usize,
    file_cache: &mut HashMap<String, Vec<String>>,
    highlight_with_target_name: bool,
) {
    for group in groups {
        let file = group.symbol.file.display().to_string();
        let kind = group
            .symbol
            .kind
            .map(|k| format!("{:?}", k).to_lowercase())
            .unwrap_or_else(|| "symbol".to_string());

        let first_line = group.call_sites.first().map(|s| s.line).unwrap_or(0);
        println!(
            "{label}: {} ({kind})  [{file}:{first_line}]",
            group.symbol.name
        );

        let Some(lines) = load_file_lines(&file, file_cache) else {
            continue;
        };

        if max_lines_per_block == 0 {
            continue;
        }

        let mut windows: Vec<(u32, u32)> = Vec::new();
        let mut call_columns: HashMap<u32, u32> = HashMap::new();

        for site in &group.call_sites {
            if site.line == 0 {
                continue;
            }

            let total_lines = lines.len() as u32;
            if total_lines == 0 || site.line > total_lines {
                continue;
            }

            let line_no = site.line;
            let start = line_no.saturating_sub(context).max(1);
            let end = (line_no + context).min(total_lines);
            windows.push((start, end));

            if let Some(col) = compute_call_column(
                lines,
                line_no,
                target_name,
                &group.symbol.name,
                highlight_with_target_name,
            ) {
                call_columns.entry(line_no).or_insert(col);
            }
        }

        if windows.is_empty() {
            continue;
        }

        windows.sort_by_key(|(start, _)| *start);
        let mut merged: Vec<(u32, u32)> = Vec::new();
        for (start, end) in windows {
            if let Some(last) = merged.last_mut() {
                if start <= last.1 + 1 {
                    last.1 = last.1.max(end);
                } else {
                    merged.push((start, end));
                }
            } else {
                merged.push((start, end));
            }
        }

        let mut printed = 0usize;

        for (start, end) in merged {
            for line_no in start..=end {
                if printed >= max_lines_per_block {
                    break;
                }

                let idx = (line_no - 1) as usize;
                if idx >= lines.len() {
                    break;
                }

                let text = &lines[idx];
                if let Some(col) = call_columns.get(&line_no).copied() {
                    println!("{line_no}:{col}:  {text}");
                } else {
                    println!("{line_no}:  {text}");
                }

                printed += 1;
            }

            if printed >= max_lines_per_block {
                break;
            }
        }

        println!();
    }
}

fn compute_call_column(
    lines: &[String],
    line_no: u32,
    target_name: &str,
    neighbor_name: &str,
    highlight_with_target_name: bool,
) -> Option<u32> {
    let idx = (line_no.saturating_sub(1)) as usize;
    if idx >= lines.len() {
        return None;
    }

    let text = &lines[idx];
    let needle = if highlight_with_target_name {
        // For callers: highlight where they call the target
        // symbol by searching for the target's name.
        target_name
    } else {
        // For callees: highlight the callee symbol being
        // invoked by searching for the neighbor's name.
        neighbor_name
    };

    text.find(needle)
        .map(|col| col as u32 + 1)
}
