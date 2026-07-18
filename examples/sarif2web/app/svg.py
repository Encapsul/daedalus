import html
import os
import re

E = html.escape


def _severity_color(level, dark=True):
    if dark:
        return {"error": "#ff5555", "warning": "#ffb86c", "note": "#8be9fd", "info": "#8be9fd"}.get(level, "#6272a4")
    return {"error": "#c0392b", "warning": "#e67e22", "note": "#2980b9", "info": "#2980b9"}.get(level, "#7f8c8d")


def _theme_palette(dark=True):
    if dark:
        return {
            "canvas_bg": "#282a36",
            "box_bg": "#44475a",
            "box_text": "#f8f8f2",
            "box_sub": "#6272a4",
            "detail_bg": "#343746",
            "detail_border": "#6272a4",
            "detail_text": "#f8f8f2",
            "detail_danger": "#ff5555",
            "file_ref": "#6272a4",
            "desc_bg": "#343746",
            "desc_border": "#6272a4",
            "desc_text": "#cdd6f4",
            "rule_color": "#bd93f9",
            "source_color": "#ff5555",
            "sink_color": "#ff5555",
            "step_color": "#8be9fd",
        }
    return {
        "canvas_bg": "#fafafa",
        "box_bg": "white",
        "box_text": "#2c3e50",
        "box_sub": "#7f8c8d",
        "detail_bg": "#f8f9fa",
        "detail_border": "#dee2e6",
        "detail_text": "#495057",
        "detail_danger": "#c0392b",
        "file_ref": "#6c757d",
        "desc_bg": "#eef2f7",
        "desc_border": "#bdc3c7",
        "desc_text": "#34495e",
        "rule_color": "#8e44ad",
        "source_color": "#e74c3c",
        "sink_color": "#c0392b",
        "step_color": "#3498db",
    }


def _cwe_short(cwe_str):
    m = re.match(r"(CWE-\d+)", cwe_str)
    return m.group(1) if m else cwe_str[:20]


def _rule_short_name(rule_id):
    parts = rule_id.split(".")
    return parts[-1] if parts else rule_id


def _wrap_text(text, width=60):
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    out = []
    for raw_line in text.split("\n"):
        line = raw_line
        while len(line) > width:
            idx = line.rfind(" ", 0, width)
            if idx <= 0:
                idx = width
            out.append(line[:idx])
            line = line[idx:].lstrip()
        out.append(line)
    return out[:20]


def render_finding_svg(finding, finding_index=0, dark=True):
    elements = []
    chain_id = f"f{finding_index}"
    pal = _theme_palette(dark)

    CANVAS_W = 1240
    BOX_W = 560
    DETAIL_W = 560
    BOX_H = 80
    GAP_Y = 34
    MARGIN_X = 34
    LEFT_X = MARGIN_X
    RIGHT_X = MARGIN_X + BOX_W + 24
    ARROW_X = LEFT_X + BOX_W // 2

    sev_color = _severity_color(finding["level"], dark)
    cwe_label = ", ".join(_cwe_short(c) for c in finding.get("cwes", [])) if finding.get("cwes") else "N/A"
    locations = finding.get("all_locations", [])
    n_locations = len(locations)

    source_tool = finding.get("source_tool", "")
    tool_badge = f"  [{source_tool}]" if source_tool else ""

    elements.append(f'''
        <defs>
            <marker id="ah-{chain_id}" viewBox="0 0 10 10" refX="9" refY="5"
                    markerWidth="7" markerHeight="5" orient="auto">
                <path d="M0 0 L10 5 L0 10z" fill="{sev_color}"/>
            </marker>
        </defs>''')

    title_text = f"[{finding['level'].upper()}] {finding['rule_id']}"
    count_text = f"x{n_locations}" if n_locations > 1 else ""

    elements.append(f'''
        <rect x="10" y="10" width="{CANVAS_W - 20}" height="38" rx="5"
              fill="{sev_color}" opacity="0.9"/>
        <text x="22" y="34" font-size="14" font-weight="bold" fill="white"
              font-family="monospace">{E(title_text)}{E(tool_badge)}</text>
        <text x="{CANVAS_W - 30}" y="34" font-size="12" fill="white" text-anchor="end"
              font-family="monospace">{E(cwe_label)}  {E(count_text)}</text>''')

    cursor_y = 58

    def _draw_step(label, label_color, title, subtitle, detail_text, file_ref, y):
        parts = []
        detail_lines = _wrap_text(detail_text, 62)
        detail_h = max(BOX_H, len(detail_lines) * 16 + 30)
        row_h = max(BOX_H, detail_h)

        parts.append(f'''
        <g transform="translate({LEFT_X}, {y})">
            <rect width="{BOX_W}" height="24" rx="3" fill="{label_color}" opacity="0.85"/>
            <text x="{BOX_W // 2}" y="16" text-anchor="middle" font-size="11" fill="white"
                  font-weight="bold" font-family="monospace">{E(label)}</text>
            <rect y="24" width="{BOX_W}" height="{row_h - 24}" fill="{pal['box_bg']}"
                  stroke="{label_color}" stroke-width="2"/>
            <text x="{BOX_W // 2}" y="46" text-anchor="middle" font-size="13"
                  font-weight="bold" fill="{pal['box_text']}" font-family="monospace">{E(title)}</text>
            <text x="{BOX_W // 2}" y="64" text-anchor="middle" font-size="11"
                  fill="{pal['box_sub']}" font-family="monospace">{E(subtitle)}</text>
        </g>''')

        parts.append(f'''
        <g transform="translate({RIGHT_X}, {y})">
            <rect width="{DETAIL_W}" height="{row_h}" rx="4" fill="{pal['detail_bg']}"
                  stroke="{pal['detail_border']}" stroke-width="1" stroke-dasharray="4,2"/>''')
        for li, line in enumerate(detail_lines):
            clr = pal["detail_danger"] if any(kw in line.lower() for kw in [
                "danger", "vulner", "inject", "unsanit", "malicious",
            ]) else pal["detail_text"]
            parts.append(f'''
            <text x="12" y="{18 + li * 16}" font-size="11" fill="{clr}"
                  font-family="monospace">{E(line)}</text>''')

        file_display = file_ref
        if len(file_display) > 64:
            file_display = "..." + file_display[-61:]
        parts.append(f'''
            <text x="12" y="{row_h - 6}" font-size="10" fill="{pal['file_ref']}"
                  font-style="italic" font-family="monospace">{E(file_display)}</text>
        </g>''')

        return "\n".join(parts), row_h

    # RULE step
    rule_svg, rule_h = _draw_step(
        label="RULE", label_color=pal["rule_color"],
        title=_rule_short_name(finding["rule_id"]),
        subtitle=cwe_label,
        detail_text=(finding.get("message") or "")[:250],
        file_ref=finding.get("help_uri") or finding["rule_id"],
        y=cursor_y,
    )
    elements.append(rule_svg)
    cursor_y = cursor_y + rule_h + GAP_Y

    code_flows = finding.get("code_flows", [])

    if code_flows:
        flow = code_flows[0]
        for fi, fstep in enumerate(flow):
            is_first = (fi == 0)
            is_last = (fi == len(flow) - 1)
            label = "SOURCE" if is_first else ("SINK" if is_last else f"STEP {fi}")
            color = pal["source_color"] if is_first else (pal["sink_color"] if is_last else pal["step_color"])

            elements.append(f'''
        <line x1="{ARROW_X}" y1="{cursor_y - GAP_Y + 3}" x2="{ARROW_X}" y2="{cursor_y - 3}"
              stroke="{sev_color}" stroke-width="2" marker-end="url(#ah-{chain_id})"/>''')

            step_svg, step_h = _draw_step(
                label=label, label_color=color,
                title=os.path.basename(fstep["file"]),
                subtitle=f"L{fstep['line']}",
                detail_text=fstep["message"] or fstep["snippet"],
                file_ref=f"{fstep['file']}:{fstep['line']}",
                y=cursor_y,
            )
            elements.append(step_svg)
            cursor_y += step_h + GAP_Y

    elif n_locations <= 3:
        for li, loc in enumerate(locations):
            elements.append(f'''
        <line x1="{ARROW_X}" y1="{cursor_y - GAP_Y + 3}" x2="{ARROW_X}" y2="{cursor_y - 3}"
              stroke="{sev_color}" stroke-width="2" marker-end="url(#ah-{chain_id})"/>''')

            step_svg, step_h = _draw_step(
                label=f"FINDING {li + 1}/{n_locations}" if n_locations > 1 else "FINDING",
                label_color=sev_color,
                title=os.path.basename(loc["file"]),
                subtitle=f"L{loc['start_line']}-L{loc['end_line']}  C{loc.get('start_col', 0)}-C{loc.get('end_col', 0)}",
                detail_text=loc["snippet"],
                file_ref=f"{loc['file']}:{loc['start_line']}",
                y=cursor_y,
            )
            elements.append(step_svg)
            cursor_y += step_h + GAP_Y
    else:
        elements.append(f'''
        <line x1="{ARROW_X}" y1="{cursor_y - GAP_Y + 3}" x2="{ARROW_X}" y2="{cursor_y - 3}"
              stroke="{sev_color}" stroke-width="2" marker-end="url(#ah-{chain_id})"/>''')

        file_list_lines = [f"{loc['file']}:{loc['start_line']}" for loc in locations]
        snippet = locations[0]["snippet"] if locations else ""

        step_svg, step_h = _draw_step(
            label=f"FINDINGS ({n_locations} occurrences)", label_color=sev_color,
            title="Multiple files",
            subtitle=f"{n_locations} locations with same pattern",
            detail_text=snippet + "\n\n" + "\n".join(file_list_lines),
            file_ref=f"{n_locations} files affected",
            y=cursor_y,
        )
        elements.append(step_svg)
        cursor_y += step_h + GAP_Y

    # Description box
    desc = finding.get("description") or finding.get("message") or ""
    if finding.get("help_uri"):
        desc += f"\n\nRef: {finding['help_uri']}"
    if finding.get("tags"):
        desc += f"\nTags: {', '.join(finding['tags'][:10])}"
    desc_lines = _wrap_text(desc, 128)
    desc_h = len(desc_lines) * 16 + 22
    full_w = CANVAS_W - MARGIN_X * 2

    elements.append(f'''
        <g transform="translate({MARGIN_X}, {cursor_y})">
            <rect width="{full_w}" height="{desc_h}" rx="5"
                  fill="{pal['desc_bg']}" stroke="{pal['desc_border']}" stroke-width="1"/>''')
    for li, line in enumerate(desc_lines):
        elements.append(f'''
            <text x="12" y="{18 + li * 16}" font-size="11" fill="{pal['desc_text']}"
                  font-family="monospace">{E(line)}</text>''')
    elements.append("</g>")

    cursor_y += desc_h + 20

    svg_html = f'''<svg xmlns="http://www.w3.org/2000/svg"
         width="{CANVAS_W + 40}" height="{cursor_y}"
         viewBox="0 0 {CANVAS_W + 40} {cursor_y}">
        <rect width="100%" height="100%" fill="{pal['canvas_bg']}" rx="6"/>
        {"".join(elements)}
    </svg>'''
    return svg_html
