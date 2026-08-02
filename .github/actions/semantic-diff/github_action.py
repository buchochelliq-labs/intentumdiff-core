"""GitHub Action helper for IntentDiff PR checks."""

from __future__ import annotations

import argparse
import html
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path
from typing import Any

from intentdiff import CommitDiffer
from intentdiff.analysis.guardrail_reports import (
    build_guardrail_check_result,
    render_guardrail_annotations,
    render_guardrail_json,
    render_guardrail_sarif,
)
from intentdiff.core.config import find_intentdiff_config, load_project_diff_config
from intentdiff.core.models import CommitDiff, DiffConfig, SemanticDiff

COMMENT_MARKER = "<!-- intentdiff:summary -->"
DEFAULT_REPORT_DIR = "intentdiff-report"
SEMANTIC_JSON = "semantic-diff.json"
GUARDRAILS_JSON = "guardrails.json"
GUARDRAILS_SARIF = "guardrails.sarif"
SUMMARY_MD = "summary.md"
HTML_REPORT = "intentdiff-review.html"


@dataclass(frozen=True)
class ActionOptions:
    repo: Path
    base_ref: str
    head_ref: str
    policy: str
    strict: bool
    fuel: int | None
    paths: tuple[str, ...]
    comment: bool
    github_token: str
    upload_sarif: bool
    upload_artifact: bool
    artifact_name: str
    fail_on_semantic_change: bool
    report_dir: Path


@dataclass(frozen=True)
class ActionSummary:
    semantic_changes: int
    style_only_changes: int
    guardrail_violations: int
    immutable_violations: int
    important_violations: int
    cross_file_changes: int
    checked_files: int
    passed: bool
    exit_code: int
    json_path: Path
    sarif_path: Path
    summary_path: Path
    html_path: Path


ApiRequest = Callable[[str, str, str, Mapping[str, Any] | None], Any]


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Run IntentDiff as a GitHub Action.")
    parser.add_argument(
        "--repo",
        default=os.environ.get("GITHUB_WORKSPACE", "."),
        help="Repository checkout path.",
    )
    args = parser.parse_args(argv)
    options = options_from_env(os.environ, repo=Path(args.repo))
    return run_action(options, env=os.environ)


def options_from_env(env: Mapping[str, str], *, repo: Path | None = None) -> ActionOptions:
    event = _load_event(env)
    base_ref, head_ref = _resolve_refs(env, event)
    report_dir_text = _env(env, "INTENTDIFF_ACTION_REPORT_DIR", DEFAULT_REPORT_DIR)
    return ActionOptions(
        repo=repo or Path(_env(env, "GITHUB_WORKSPACE", ".")),
        base_ref=_env(env, "INTENTDIFF_ACTION_BASE_REF", base_ref),
        head_ref=_env(env, "INTENTDIFF_ACTION_HEAD_REF", head_ref),
        policy=_env(env, "INTENTDIFF_ACTION_POLICY", ""),
        strict=parse_bool(_env(env, "INTENTDIFF_ACTION_STRICT", "false")),
        fuel=_parse_optional_int(_env(env, "INTENTDIFF_ACTION_FUEL", "")),
        paths=parse_paths(_env(env, "INTENTDIFF_ACTION_PATHS", "")),
        comment=parse_bool(_env(env, "INTENTDIFF_ACTION_COMMENT", "false")),
        github_token=_env(env, "INTENTDIFF_ACTION_GITHUB_TOKEN", ""),
        upload_sarif=parse_bool(_env(env, "INTENTDIFF_ACTION_UPLOAD_SARIF", "true")),
        upload_artifact=parse_bool(_env(env, "INTENTDIFF_ACTION_UPLOAD_ARTIFACT", "true")),
        artifact_name=_env(env, "INTENTDIFF_ACTION_ARTIFACT_NAME", "semantic-diff-report"),
        fail_on_semantic_change=parse_bool(
            _env(env, "INTENTDIFF_ACTION_FAIL_ON_SEMANTIC_CHANGE", "false")
        ),
        report_dir=Path(report_dir_text),
    )


def parse_bool(value: str | bool | None) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    return value.strip().lower() in {"1", "true", "yes", "y", "on"}


def parse_paths(value: str) -> tuple[str, ...]:
    parts: list[str] = []
    for line in value.replace(",", "\n").splitlines():
        item = line.strip()
        if item:
            parts.append(item)
    return tuple(parts)


def run_action(
    options: ActionOptions,
    *,
    env: Mapping[str, str],
    differ_factory: Callable[[DiffConfig], CommitDiffer] = CommitDiffer,
    api_request: ApiRequest | None = None,
) -> int:
    config = load_project_diff_config(options.repo)
    if options.fuel is not None:
        config.plugin_fuel = options.fuel
    if options.policy:
        config.guardrail_policy_path = _policy_path(options.repo, options.policy)
    else:
        config.guardrail_policy_path = find_intentdiff_config(options.repo)
    config.guardrails_strict = options.strict

    commit_diff = differ_factory(config).diff_commit(
        repo_path=options.repo,
        old_ref=options.base_ref,
        new_ref=options.head_ref,
    )
    commit_diff = _filter_commit_diff(commit_diff, options.paths)
    result = _write_reports(commit_diff, options)
    _write_step_outputs(result, options, env)
    _write_step_summary(result, env)

    annotations = render_guardrail_annotations(
        build_guardrail_check_result(
            commit_diff.guardrail_violations,
            checked_files=result.checked_files,
            strict=options.strict,
        )
    )
    if annotations:
        print(annotations)

    if options.comment:
        _upsert_pr_comment(
            result.summary_path.read_text(encoding="utf-8"),
            env=env,
            token=options.github_token,
            api_request=api_request or _github_api_request,
        )

    return result.exit_code


def summarize_commit_diff(
    commit_diff: CommitDiff,
    *,
    strict: bool,
    fail_on_semantic_change: bool,
    report_dir: Path,
) -> ActionSummary:
    guardrail_result = build_guardrail_check_result(
        commit_diff.guardrail_violations,
        checked_files=len(commit_diff.file_diffs),
        strict=strict,
    )
    semantic_changes = sum(
        diff.has_semantic_changes and not diff.is_style_only
        for diff in commit_diff.file_diffs
    )
    style_only_changes = sum(diff.is_style_only for diff in commit_diff.file_diffs)
    semantic_failure = fail_on_semantic_change and semantic_changes > 0
    if guardrail_result.immutable_count and strict:
        exit_code = 2
    elif semantic_failure:
        exit_code = 3
    else:
        exit_code = 0

    return ActionSummary(
        semantic_changes=semantic_changes,
        style_only_changes=style_only_changes,
        guardrail_violations=guardrail_result.violation_count,
        immutable_violations=guardrail_result.immutable_count,
        important_violations=guardrail_result.important_count,
        cross_file_changes=len(commit_diff.cross_file_changes),
        checked_files=len(commit_diff.file_diffs),
        passed=exit_code == 0,
        exit_code=exit_code,
        json_path=report_dir / SEMANTIC_JSON,
        sarif_path=report_dir / GUARDRAILS_SARIF,
        summary_path=report_dir / SUMMARY_MD,
        html_path=report_dir / HTML_REPORT,
    )


def render_summary_markdown(commit_diff: CommitDiff, summary: ActionSummary) -> str:
    status = "passed" if summary.passed else "needs attention"
    lines = [
        COMMENT_MARKER,
        "# IntentDiff PR Guard",
        "",
        f"Status: **{status}**",
        "",
        "| Metric | Count |",
        "|---|---:|",
        f"| Files checked | {summary.checked_files} |",
        f"| Semantic changes | {summary.semantic_changes} |",
        f"| Style-only changes | {summary.style_only_changes} |",
        f"| Guardrail violations | {summary.guardrail_violations} |",
        f"| Immutable guardrails | {summary.immutable_violations} |",
        f"| Important guardrails | {summary.important_violations} |",
        f"| Cross-file changes | {summary.cross_file_changes} |",
        "",
    ]
    if commit_diff.guardrail_violations:
        lines.extend(["## Protected Config Changes", ""])
        for violation in commit_diff.guardrail_violations[:20]:
            lines.append(
                f"- **{violation.severity.value}** `{violation.semantic_path}` "
                f"in `{violation.file}`: {violation.message}"
            )
        lines.append("")
    if commit_diff.cross_file_changes:
        lines.extend(["## Cross-File Changes", ""])
        for change in commit_diff.cross_file_changes[:20]:
            lines.append(
                f"- **{change.change_type}** `{change.symbol_name}`: "
                f"`{change.old_file}` -> `{change.new_file}`"
            )
        lines.append("")
    lines.append(
        "Artifacts include `semantic-diff.json`, `guardrails.json`, "
        "`guardrails.sarif`, and `intentdiff-review.html`."
    )
    return "\n".join(lines) + "\n"


def render_static_html_report(commit_diff: CommitDiff, summary: ActionSummary) -> str:
    """Render a static PR review artifact without remote assets or scripts."""

    file_cards = "\n".join(_render_html_file_card(diff) for diff in commit_diff.file_diffs)
    guardrails = "\n".join(
        (
            "<li>"
            f"<strong>{_h(violation.severity.value)}</strong> "
            f"<code>{_h(violation.file)}</code> "
            f"<span>{_h(violation.semantic_path)}</span> "
            f"<em>{_h(violation.message)}</em>"
            "</li>"
        )
        for violation in commit_diff.guardrail_violations[:50]
    )
    cross_file = "\n".join(
        (
            "<li>"
            f"<strong>{_h(str(change.change_type))}</strong> "
            f"<code>{_h(change.symbol_name)}</code> "
            f"<span>{_h(change.old_file)} -> {_h(change.new_file)}</span>"
            "</li>"
        )
        for change in commit_diff.cross_file_changes[:50]
    )
    status_class = "ok" if summary.passed else "attention"
    status_text = "Passed" if summary.passed else "Needs attention"
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>IntentDiff PR Review</title>
  <style>
    :root {{
      color-scheme: dark;
      --bg: #061120;
      --panel: #0d1b2f;
      --panel-2: #102540;
      --line: #22415f;
      --text: #e7f7ff;
      --muted: #9fc1d6;
      --mint: #4ee7c6;
      --cyan: #45d9ff;
      --amber: #f4c95d;
      --red: #ff6b7d;
      --green: #68df8e;
      --purple: #b98cff;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      background: radial-gradient(circle at 20% 0%, rgba(69, 217, 255, .16), transparent 30%), var(--bg);
      color: var(--text);
      font: 14px/1.5 ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }}
    main {{ max-width: 1180px; margin: 0 auto; padding: 28px; }}
    header {{
      border: 1px solid var(--line);
      background: linear-gradient(135deg, rgba(13, 27, 47, .96), rgba(16, 37, 64, .88));
      border-radius: 14px;
      padding: 24px;
      box-shadow: 0 24px 80px rgba(0, 0, 0, .28);
    }}
    .eyebrow {{ color: var(--cyan); letter-spacing: .08em; text-transform: uppercase; font-size: 12px; }}
    h1 {{ margin: 6px 0 8px; font-size: 34px; line-height: 1.1; }}
    .status {{ display: inline-flex; align-items: center; gap: 8px; padding: 6px 12px; border-radius: 999px; border: 1px solid; }}
    .status.ok {{ color: var(--green); border-color: rgba(104, 223, 142, .6); background: rgba(104, 223, 142, .12); }}
    .status.attention {{ color: var(--amber); border-color: rgba(244, 201, 93, .6); background: rgba(244, 201, 93, .12); }}
    .metrics {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: 10px; margin: 18px 0 0; }}
    .metric {{ border: 1px solid var(--line); border-radius: 12px; background: rgba(6, 17, 32, .45); padding: 12px; }}
    .metric strong {{ display: block; font-size: 24px; color: var(--mint); }}
    .metric span {{ color: var(--muted); font-size: 12px; text-transform: uppercase; letter-spacing: .06em; }}
    section {{ margin-top: 18px; }}
    .section-title {{ color: var(--cyan); margin: 0 0 10px; font-size: 13px; letter-spacing: .08em; text-transform: uppercase; }}
    .grid {{ display: grid; grid-template-columns: minmax(0, 1fr); gap: 12px; }}
    .file-card, .list-panel {{
      border: 1px solid var(--line);
      border-radius: 12px;
      background: rgba(13, 27, 47, .9);
      overflow: hidden;
    }}
    .file-head {{
      display: flex; justify-content: space-between; gap: 12px; align-items: start;
      padding: 14px 16px; background: rgba(16, 37, 64, .78); border-bottom: 1px solid var(--line);
    }}
    .file-path {{ font-weight: 700; font-size: 16px; word-break: break-word; }}
    .badges {{ display: flex; flex-wrap: wrap; gap: 6px; margin-top: 6px; }}
    .badge {{
      border: 1px solid var(--line); border-radius: 999px; padding: 3px 8px;
      color: var(--muted); background: rgba(6, 17, 32, .5); font-size: 12px;
    }}
    .badge.semantic {{ color: var(--cyan); border-color: rgba(69, 217, 255, .55); }}
    .badge.guardrail {{ color: var(--red); border-color: rgba(255, 107, 125, .58); }}
    .badge.style {{ color: var(--amber); border-color: rgba(244, 201, 93, .55); }}
    .changes {{ padding: 12px 16px; display: grid; gap: 8px; }}
    .change {{ display: grid; gap: 3px; border-left: 3px solid var(--cyan); padding-left: 10px; }}
    .change.addition {{ border-color: var(--green); }}
    .change.deletion {{ border-color: var(--red); }}
    .change.modification {{ border-color: var(--amber); }}
    .change.refactoring {{ border-color: var(--purple); }}
    .change small {{ color: var(--muted); }}
    ul {{ margin: 0; padding: 12px 18px 16px 30px; }}
    li {{ margin: 6px 0; }}
    code {{ color: var(--mint); }}
    em {{ color: var(--muted); font-style: normal; }}
  </style>
</head>
<body>
<main>
  <header>
    <div class="eyebrow">IntentDiff PR Review</div>
    <h1>Semantic review artifact</h1>
    <div class="status {status_class}">{_h(status_text)}</div>
    <div class="metrics">
      {_metric("Files", summary.checked_files)}
      {_metric("Semantic", summary.semantic_changes)}
      {_metric("Style-only", summary.style_only_changes)}
      {_metric("Guardrails", summary.guardrail_violations)}
      {_metric("Immutable", summary.immutable_violations)}
      {_metric("Cross-file", summary.cross_file_changes)}
    </div>
  </header>
  <section>
    <h2 class="section-title">Changed files</h2>
    <div class="grid">{file_cards or '<div class="list-panel"><ul><li>No changed files matched the Action filters.</li></ul></div>'}</div>
  </section>
  <section>
    <h2 class="section-title">Guardrails</h2>
    <div class="list-panel"><ul>{guardrails or '<li>No guardrail violations.</li>'}</ul></div>
  </section>
  <section>
    <h2 class="section-title">Cross-file changes</h2>
    <div class="list-panel"><ul>{cross_file or '<li>No cross-file changes detected.</li>'}</ul></div>
  </section>
</main>
</body>
</html>
"""


def _render_html_file_card(diff: SemanticDiff) -> str:
    filename = diff.new_filename or diff.old_filename
    schema = _schema_label(diff)
    badges = [
        f'<span class="badge">{_h(diff.language)}</span>',
        f'<span class="badge semantic">{len(diff.change_groups)} groups</span>',
        f'<span class="badge">{len(diff.changes)} raw changes</span>',
    ]
    if schema:
        badges.append(f'<span class="badge semantic">{_h(schema)}</span>')
    if diff.guardrail_violations:
        badges.append(
            f'<span class="badge guardrail">{len(diff.guardrail_violations)} guardrails</span>'
        )
    if diff.is_style_only:
        badges.append('<span class="badge style">style-only</span>')
    changes = "\n".join(_render_html_change(change) for change in diff.changes[:8])
    if len(diff.changes) > 8:
        changes += f'\n<div class="change"><small>{len(diff.changes) - 8} more raw changes in semantic-diff.json</small></div>'
    return (
        '<article class="file-card">'
        '<div class="file-head">'
        f'<div><div class="file-path">{_h(filename)}</div><div class="badges">{"".join(badges)}</div></div>'
        f'<div class="badge">{"semantic" if diff.has_semantic_changes else "clean/style"}</div>'
        '</div>'
        f'<div class="changes">{changes or "<div class=\"change\"><small>No raw changes.</small></div>"}</div>'
        '</article>'
    )


def _render_html_change(change: Any) -> str:
    change_type = str(getattr(change, "change_type", "change"))
    change_class = change_type.lower().replace("_", "-")
    if "add" in change_class:
        css = "addition"
    elif "delete" in change_class:
        css = "deletion"
    elif "refactor" in change_class:
        css = "refactoring"
    else:
        css = "modification"
    description = str(getattr(change, "description", "") or change_type)
    old_label = _node_label(getattr(change, "old_node", None))
    new_label = _node_label(getattr(change, "new_node", None))
    labels = " -> ".join(label for label in (old_label, new_label) if label)
    return (
        f'<div class="change {css}">'
        f'<strong>{_h(change_type)}</strong>'
        f'<small>{_h(description)}</small>'
        f'<small>{_h(labels)}</small>'
        '</div>'
    )


def _schema_label(diff: SemanticDiff) -> str:
    schema = dict(diff.metadata.get("schema", {})) if isinstance(diff.metadata, Mapping) else {}
    if not schema:
        return ""
    provider = str(schema.get("provider_id") or "schema")
    status = str(schema.get("status") or "")
    available = schema.get("available")
    if available is False:
        return f"{provider} unavailable"
    return f"{provider} {status}".strip()


def _node_label(node: Any) -> str:
    if node is None:
        return ""
    label = str(getattr(node, "label", "") or "")
    node_type = str(getattr(node, "node_type", "") or "")
    return f"{node_type}({label})" if label else node_type


def _metric(label: str, value: int) -> str:
    return f'<div class="metric"><strong>{value}</strong><span>{_h(label)}</span></div>'


def _h(value: object) -> str:
    return html.escape(str(value), quote=True)


def _write_reports(commit_diff: CommitDiff, options: ActionOptions) -> ActionSummary:
    report_dir = options.report_dir
    report_dir.mkdir(parents=True, exist_ok=True)
    summary = summarize_commit_diff(
        commit_diff,
        strict=options.strict,
        fail_on_semantic_change=options.fail_on_semantic_change,
        report_dir=report_dir,
    )
    guardrail_result = build_guardrail_check_result(
        commit_diff.guardrail_violations,
        checked_files=summary.checked_files,
        strict=options.strict,
    )
    summary.json_path.write_text(
        commit_diff.model_dump_json(indent=2),
        encoding="utf-8",
    )
    (report_dir / GUARDRAILS_JSON).write_text(
        render_guardrail_json(guardrail_result),
        encoding="utf-8",
    )
    summary.sarif_path.write_text(
        render_guardrail_sarif(guardrail_result),
        encoding="utf-8",
    )
    summary.summary_path.write_text(
        render_summary_markdown(commit_diff, summary),
        encoding="utf-8",
    )
    summary.html_path.write_text(
        render_static_html_report(commit_diff, summary),
        encoding="utf-8",
    )
    return summary


def _filter_commit_diff(commit_diff: CommitDiff, patterns: tuple[str, ...]) -> CommitDiff:
    if not patterns:
        return commit_diff
    file_diffs = [
        diff for diff in commit_diff.file_diffs
        if _matches_any(diff.old_filename, diff.new_filename, patterns)
    ]
    allowed_files = {
        name
        for diff in file_diffs
        for name in (diff.old_filename, diff.new_filename)
        if name
    }
    return commit_diff.model_copy(
        update={
            "file_diffs": file_diffs,
            "guardrail_violations": [
                violation
                for violation in commit_diff.guardrail_violations
                if _matches_path(violation.file, patterns)
            ],
            "cross_file_changes": [
                change
                for change in commit_diff.cross_file_changes
                if change.old_file in allowed_files or change.new_file in allowed_files
            ],
        }
    )


def _matches_any(old_filename: str, new_filename: str, patterns: tuple[str, ...]) -> bool:
    return _matches_path(old_filename, patterns) or _matches_path(new_filename, patterns)


def _matches_path(path: str, patterns: tuple[str, ...]) -> bool:
    normalised = path.replace("\\", "/")
    return any(fnmatch(normalised, pattern) for pattern in patterns)


def _policy_path(repo: Path, value: str) -> Path:
    path = Path(value)
    return path if path.is_absolute() else repo / path


def _write_step_outputs(
    summary: ActionSummary,
    options: ActionOptions,
    env: Mapping[str, str],
) -> None:
    output_path = env.get("GITHUB_OUTPUT")
    if not output_path:
        return
    values = {
        "passed": str(summary.passed).lower(),
        "semantic-changes": str(summary.semantic_changes),
        "style-only-changes": str(summary.style_only_changes),
        "guardrail-violations": str(summary.guardrail_violations),
        "immutable-violations": str(summary.immutable_violations),
        "important-violations": str(summary.important_violations),
        "cross-file-changes": str(summary.cross_file_changes),
        "report-dir": str(options.report_dir),
        "json-path": str(summary.json_path),
        "sarif-path": str(summary.sarif_path),
        "summary-path": str(summary.summary_path),
        "html-path": str(summary.html_path),
        "exit-code": str(summary.exit_code),
        "upload-sarif": str(options.upload_sarif).lower(),
        "upload-artifact": str(options.upload_artifact).lower(),
        "artifact-name": options.artifact_name,
    }
    with Path(output_path).open("a", encoding="utf-8") as handle:
        for key, value in values.items():
            handle.write(f"{key}={value}\n")


def _write_step_summary(summary: ActionSummary, env: Mapping[str, str]) -> None:
    step_summary = env.get("GITHUB_STEP_SUMMARY")
    if not step_summary:
        return
    text = summary.summary_path.read_text(encoding="utf-8")
    with Path(step_summary).open("a", encoding="utf-8") as handle:
        handle.write(text)


def _resolve_refs(
    env: Mapping[str, str],
    event: Mapping[str, Any],
) -> tuple[str, str]:
    pull_request = event.get("pull_request")
    if isinstance(pull_request, dict):
        base = pull_request.get("base", {})
        head = pull_request.get("head", {})
        base_sha = base.get("sha") if isinstance(base, dict) else None
        head_sha = head.get("sha") if isinstance(head, dict) else None
        if base_sha and head_sha:
            return str(base_sha), str(head_sha)
    before = env.get("GITHUB_EVENT_BEFORE") or event.get("before")
    after = env.get("GITHUB_SHA") or event.get("after")
    return str(before or "HEAD~1"), str(after or "HEAD")


def _load_event(env: Mapping[str, str]) -> Mapping[str, Any]:
    event_path = env.get("GITHUB_EVENT_PATH")
    if not event_path:
        return {}
    path = Path(event_path)
    if not path.exists():
        return {}
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}
    return raw if isinstance(raw, dict) else {}


def _upsert_pr_comment(
    summary_markdown: str,
    *,
    env: Mapping[str, str],
    token: str,
    api_request: ApiRequest,
) -> None:
    if not token:
        print("::warning::comment requested but github-token is empty")
        return
    event = _load_event(env)
    pull_request = event.get("pull_request")
    if not isinstance(pull_request, dict):
        print("::warning::comment requested outside a pull_request event; skipping")
        return
    pr_number = pull_request.get("number")
    repository = env.get("GITHUB_REPOSITORY", "")
    if not pr_number or "/" not in repository:
        print("::warning::missing pull request metadata; skipping comment")
        return

    base = f"https://api.github.com/repos/{repository}/issues/{pr_number}/comments"
    comments = api_request("GET", base, token, None)
    existing_id = None
    if isinstance(comments, list):
        for comment in comments:
            body = str(comment.get("body", "")) if isinstance(comment, dict) else ""
            if COMMENT_MARKER in body:
                existing_id = comment.get("id")
                break

    payload = {"body": summary_markdown}
    if existing_id is not None:
        api_request("PATCH", f"{base}/{existing_id}", token, payload)
    else:
        api_request("POST", base, token, payload)


def _github_api_request(
    method: str,
    url: str,
    token: str,
    payload: Mapping[str, Any] | None,
) -> Any:
    _validate_github_api_url(url)
    body = None if payload is None else json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(  # noqa: S310 - URL is validated above.
        url,
        data=body,
        method=method,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
            "X-GitHub-Api-Version": "2022-11-28",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=15) as response:  # noqa: S310
            text = response.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", errors="replace")
        print(f"::warning::GitHub API {method} {url} failed: {exc.code} {detail}")
        return None
    except urllib.error.URLError as exc:
        print(f"::warning::GitHub API {method} {url} failed: {exc.reason}")
        return None
    if not text:
        return None
    return json.loads(text)


def _validate_github_api_url(url: str) -> None:
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme != "https" or parsed.netloc != "api.github.com":
        raise ValueError("GitHub API URL must use https://api.github.com")
    if not parsed.path.startswith("/repos/"):
        raise ValueError("GitHub API URL must target the repositories API")


def _env(env: Mapping[str, str], key: str, default: str) -> str:
    value = env.get(key)
    return default if value is None or value == "" else value


def _parse_optional_int(value: str) -> int | None:
    stripped = value.strip()
    if not stripped:
        return None
    if stripped.lower() in {"inf", "infinite", "unlimited"}:
        return -1
    return int(stripped)


if __name__ == "__main__":
    sys.exit(main())
