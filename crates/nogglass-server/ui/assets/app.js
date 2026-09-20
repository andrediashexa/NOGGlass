// The interface: router selector, query form, AS-PATH graph, route table.
//
// No framework and no build step. The page is embedded in the binary, so every
// kilobyte here ships with the product and has to earn its place.
//
// Two rules from the architecture decisions show up everywhere in this file:
// every string comes from a message catalogue (ADR-0004), and a value the
// router did not report renders as unknown rather than as a default (ADR-0006).

const LOCALES = ["pt", "en", "es"];
const CATALOGUE_FILE = { pt: "pt-BR", en: "en", es: "es" };

const state = {
  locale: document.documentElement.dataset.locale || "en",
  messages: {},
  fallback: {},
  routers: [],
};

/** Translates a key, falling back to English rather than showing a raw key. */
function t(key) {
  return state.messages[key] ?? state.fallback[key] ?? key;
}

async function loadMessages(locale) {
  const file = CATALOGUE_FILE[locale] ?? "en";
  const [messages, fallback] = await Promise.all([
    fetch(`/assets/messages/${file}.json`).then((r) => r.json()),
    file === "en"
      ? Promise.resolve(null)
      : fetch("/assets/messages/en.json").then((r) => r.json()),
  ]);
  state.messages = messages;
  state.fallback = fallback ?? messages;
}

function applyTranslations() {
  for (const node of document.querySelectorAll("[data-i18n]")) {
    node.textContent = t(node.dataset.i18n);
  }
  const target = document.getElementById("target");
  target.placeholder = t("form.target.placeholder");
  document.title = `${t("app.name")} — ${t("app.tagline")}`;
  for (const link of document.querySelectorAll("[data-locale-link]")) {
    link.setAttribute(
      "aria-current",
      link.dataset.localeLink === state.locale ? "true" : "false",
    );
  }
}

/** Renders text, or the "not reported" marker when a value is absent. */
function valueOrUnknown(cell, value, render = String) {
  if (value === null || value === undefined) {
    cell.textContent = t("result.unknown");
    cell.className = "unknown";
  } else {
    cell.textContent = render(value);
  }
}

async function loadRouters() {
  const routers = await fetch("/api/routers").then((r) => r.json());
  state.routers = routers;

  const select = document.getElementById("router");
  select.replaceChildren();

  // Group by location, which is how an operator thinks about their POPs.
  const groups = new Map();
  for (const router of routers) {
    const key = router.location ?? "";
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(router);
  }

  for (const [location, members] of groups) {
    const parent = location
      ? Object.assign(document.createElement("optgroup"), { label: location })
      : select;
    for (const router of members) {
      const option = document.createElement("option");
      option.value = router.id;
      option.textContent = router.name;
      parent.appendChild(option);
    }
    if (parent !== select) select.appendChild(parent);
  }

  select.addEventListener("change", onRouterChange);
  onRouterChange();
}

function onRouterChange() {
  const router = state.routers.find(
    (r) => r.id === document.getElementById("router").value,
  );
  if (!router) return;

  // Offer only what this router answers, so the interface never presents a
  // query the server will refuse.
  const select = document.getElementById("query-type");
  select.replaceChildren();
  for (const query of router.queries) {
    const option = document.createElement("option");
    option.value = query;
    option.textContent = t(`query.${query}`);
    select.appendChild(option);
  }

  document.getElementById("mock-warning").hidden = !router.is_mock;
}

async function loadVersion() {
  try {
    const info = await fetch("/api/version").then((r) => r.json());
    document.getElementById("version").textContent = info.version;
    document.getElementById("build-meta").textContent =
      `${t("about.commit")} ${info.commit} · ${t("about.built")} ${info.built_at}`;
    const link = document.getElementById("version-link");
    link.href =
      info.version === "dev"
        ? "https://github.com/andrediashexa/looking-glass"
        : `https://github.com/andrediashexa/looking-glass/releases/tag/v${info.version}`;
  } catch {
    // The footer is not worth breaking the page over.
  }
}

function showError(code, message) {
  const box = document.getElementById("error");
  // Translated from the stable code; the server message is the fallback.
  box.textContent = state.messages[`error.${code}`] ?? message ?? t("error.unsupported");
  box.hidden = false;
}

function hidePanels() {
  for (const id of [
    "error",
    "graph-panel",
    "paths-panel",
    "raw-panel",
    "global-panel",
    "hops-panel",
  ]) {
    document.getElementById(id).hidden = true;
  }
}

async function onSubmit(event) {
  event.preventDefault();
  hidePanels();

  const button = document.getElementById("submit");
  const label = button.querySelector("span");
  button.disabled = true;
  label.textContent = t("form.running");

  const payload = {
    router: document.getElementById("router").value,
    type: document.getElementById("query-type").value,
    target: document.getElementById("target").value,
  };

  try {
    const response = await fetch("/api/query", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload),
    });
    const body = await response.json();
    if (!response.ok) {
      showError(body.code, body.message);
      return;
    }
    render(body);
  } catch (error) {
    showError("unreachable", String(error));
  } finally {
    button.disabled = false;
    label.textContent = t("form.submit");
  }
}

function render(response) {
  const meta = document.getElementById("result-meta");
  meta.textContent = `${t("result.command")}: ${response.command} · ${t("result.duration")} ${response.duration_ms} ms`;

  if (response.kind === "bgp_route") {
    renderBgp(response.result);
    renderGlobalView(response.agreement, response.global);
  } else if (response.kind === "raw") {
    renderRaw(response.result?.raw_output ?? response.output, response.truncated);
  } else if (response.kind === "traceroute") {
    renderRaw(response.result.raw_output, false);
    renderHops(response.result.hops);
  } else {
    renderRaw(response.result.raw_output, false);
  }
}

function renderRaw(output, truncated) {
  const panel = document.getElementById("raw-panel");
  const pre = document.getElementById("raw");
  pre.textContent = output ?? "";
  if (truncated) {
    pre.textContent += `\n\n${t("result.truncated")}`;
  }
  panel.hidden = false;
}

function renderBgp(result) {
  renderRaw(result.raw_output, result.truncated);

  if (result.completeness?.state === "partial") {
    const box = document.getElementById("error");
    box.textContent = t("result.partial");
    box.hidden = false;
  }

  if (!result.paths.length) {
    const box = document.getElementById("error");
    box.textContent = t("result.no_route");
    box.hidden = false;
    return;
  }

  renderTable(result.paths);
  renderGraph(result.paths);
}

/**
 * Shows what the Internet announces next to what the router answered.
 *
 * Absent when the operator did not enable the comparison; "unavailable" when
 * the lookup failed, which is not the same as agreement and must not read like
 * it.
 */
function renderGlobalView(agreement, global) {
  if (!agreement) return;

  const panel = document.getElementById("global-panel");
  const verdict = document.getElementById("global-verdict");
  const facts = document.getElementById("global-facts");
  facts.replaceChildren();

  const messages = {
    agrees: "global.agrees",
    not_announced_globally: "global.not_announced",
    not_seen_by_router: "global.not_seen_by_router",
    origin_mismatch: "global.mismatch",
    unknown: "global.unavailable",
  };

  // A comparison that was never asked for should not claim to be unavailable.
  if (agreement.state === "unknown" && !global) {
    panel.hidden = true;
    return;
  }

  verdict.textContent = t(messages[agreement.state] ?? "global.unavailable");
  verdict.className = `verdict verdict-${agreement.state === "origin_mismatch" ? "mismatch" : agreement.state}`;

  const rows = [];
  if (agreement.global_origins?.length) {
    rows.push([t("global.origins"), agreement.global_origins.map((a) => `AS${a}`).join(", ")]);
  } else if (global?.origins?.length) {
    rows.push([t("global.origins"), global.origins.map((a) => `AS${a}`).join(", ")]);
  }
  if (global?.visibility != null) rows.push([t("global.visibility"), String(global.visibility)]);
  if (global?.more_specifics) rows.push([t("global.more_specifics"), String(global.more_specifics)]);

  for (const [label, value] of rows) {
    const dt = document.createElement("dt");
    dt.textContent = label;
    const dd = document.createElement("dd");
    dd.textContent = value;
    facts.append(dt, dd);
  }

  panel.hidden = false;
}

/**
 * Renders the hop list. A hop that did not answer keeps its number and says so,
 * because where a traceroute stops is usually the answer someone came for.
 */
function renderHops(hops) {
  if (!hops?.length) return;

  const body = document.querySelector("#hops-table tbody");
  body.replaceChildren();

  for (const hop of hops) {
    const row = document.createElement("tr");

    const number = document.createElement("td");
    number.textContent = String(hop.hop);
    row.appendChild(number);

    const address = document.createElement("td");
    if (hop.ip || hop.hostname) {
      address.textContent = hop.hostname ? `${hop.hostname} (${hop.ip ?? "?"})` : hop.ip;
    } else {
      address.textContent = t("traceroute.silent");
      address.className = "unknown";
    }
    row.appendChild(address);

    const times = document.createElement("td");
    if (hop.rtt_ms?.length) {
      // Each probe, not an average: 1 ms, 1 ms and 400 ms says something an
      // average of 134 ms does not.
      times.textContent = hop.rtt_ms.map((ms) => `${ms.toFixed(3)} ms`).join("  ");
    } else {
      times.textContent = t("result.unknown");
      times.className = "unknown";
    }
    row.appendChild(times);

    body.appendChild(row);
  }

  document.getElementById("hops-panel").hidden = false;
}

function renderTable(paths) {
  const body = document.querySelector("#paths-table tbody");
  body.replaceChildren();

  for (const path of paths) {
    const row = document.createElement("tr");
    row.dataset.best = String(path.is_best);

    const cells = [
      (cell) => valueOrUnknown(cell, path.prefix),
      (cell) => valueOrUnknown(cell, path.next_hop),
      (cell) => {
        cell.textContent = path.as_path.length ? path.as_path.join(" ") : t("result.unknown");
        if (!path.as_path.length) cell.className = "unknown";
      },
      (cell) => valueOrUnknown(cell, path.local_pref),
      (cell) => valueOrUnknown(cell, path.med),
      (cell) => valueOrUnknown(cell, path.origin, (v) => v.toUpperCase()),
      (cell) => {
        const badge = document.createElement("span");
        badge.className = `badge badge-${path.rpki.status}`;
        badge.textContent = t(`rpki.${path.rpki.status}`);
        badge.title = t(`rpki.source.${path.rpki.source}`);
        cell.appendChild(badge);
      },
      (cell) => {
        if (!path.communities.length) {
          cell.textContent = t("result.unknown");
          cell.className = "unknown";
          return;
        }
        for (const community of path.communities) {
          const chip = document.createElement("span");
          chip.className = "community";
          chip.textContent = community.raw;
          if (community.name) chip.title = community.name;
          cell.appendChild(chip);
        }
      },
    ];

    for (const fill of cells) {
      const cell = document.createElement("td");
      fill(cell);
      row.appendChild(cell);
    }
    body.appendChild(row);
  }

  document.getElementById("paths-panel").hidden = false;
}

/**
 * Draws the AS paths as a graph: the local router on the left, the origin AS on
 * the right, transit in between. The best path is solid, alternatives dashed
 * (ADR-0008).
 *
 * SVG rather than canvas, so the shapes scale, stay selectable and can carry
 * titles for assistive technology.
 */
function renderGraph(paths) {
  const columns = [];
  const seen = new Map();

  for (const path of paths) {
    path.as_path.forEach((asn, index) => {
      // Depth is counted from the right, so every origin AS lands in the last
      // column however long each path is.
      const depth = path.as_path.length - index;
      const previous = seen.get(asn);
      seen.set(asn, Math.max(previous ?? 0, depth));
    });
  }

  const maxDepth = Math.max(...seen.values(), 1);
  const nodes = new Map();
  for (const [asn, depth] of seen) {
    const column = maxDepth - depth + 1;
    if (!columns[column]) columns[column] = [];
    columns[column].push(asn);
    nodes.set(asn, { asn, column });
  }
  columns[0] = ["local"];
  nodes.set("local", { asn: "local", column: 0 });

  const columnWidth = 190;
  const rowHeight = 96;
  const radius = 34;
  const height = Math.max(...columns.map((c) => c.length)) * rowHeight + 40;
  const width = columns.length * columnWidth + 60;

  for (const [index, members] of columns.entries()) {
    members.forEach((asn, row) => {
      const node = nodes.get(asn);
      node.x = 50 + index * columnWidth;
      node.y = (height / (members.length + 1)) * (row + 1);
    });
  }

  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  svg.setAttribute("width", "100%");
  svg.setAttribute("height", String(height));

  const draw = (name, attributes, parent = svg) => {
    const element = document.createElementNS("http://www.w3.org/2000/svg", name);
    for (const [key, value] of Object.entries(attributes)) {
      element.setAttribute(key, String(value));
    }
    parent.appendChild(element);
    return element;
  };

  // Edges first, so nodes sit on top of them.
  for (const path of paths) {
    const hops = ["local", ...path.as_path];
    for (let i = 0; i < hops.length - 1; i += 1) {
      const from = nodes.get(hops[i]);
      const to = nodes.get(hops[i + 1]);
      if (!from || !to) continue;
      const midpoint = (from.x + to.x) / 2;
      draw("path", {
        d: `M ${from.x + radius} ${from.y} C ${midpoint} ${from.y}, ${midpoint} ${to.y}, ${to.x - radius} ${to.y}`,
        fill: "none",
        stroke: path.is_best ? "var(--accent-best)" : "var(--accent-backup)",
        "stroke-width": path.is_best ? 3 : 1.5,
        "stroke-dasharray": path.is_best ? "none" : "6 6",
        opacity: path.is_best ? 1 : 0.55,
        class: path.is_best ? "edge-best" : "edge-backup",
      });
    }
  }

  const originAsns = new Set(
    paths.map((p) => p.as_path[p.as_path.length - 1]).filter(Boolean),
  );

  for (const node of nodes.values()) {
    const isLocal = node.asn === "local";
    const isOrigin = originAsns.has(node.asn);
    const colour = isLocal
      ? "var(--accent-local)"
      : isOrigin
        ? "var(--accent-best)"
        : "var(--accent-transit)";

    draw("circle", {
      cx: node.x,
      cy: node.y,
      r: radius,
      fill: "rgba(6, 9, 14, 0.9)",
      stroke: colour,
      "stroke-width": 2,
    });

    const label = draw("text", {
      x: node.x,
      y: node.y + 5,
      "text-anchor": "middle",
      fill: "var(--text-main)",
      "font-family": "var(--font-mono)",
      "font-size": 13,
    });
    label.textContent = isLocal ? t("form.router") : `AS${node.asn}`;
  }

  // RPKI state of the best path, next to its origin.
  const best = paths.find((p) => p.is_best) ?? paths[0];
  const originAsn = best.as_path[best.as_path.length - 1];
  const originNode = nodes.get(originAsn);
  if (originNode) {
    const badge = draw("text", {
      x: originNode.x,
      y: originNode.y + radius + 20,
      "text-anchor": "middle",
      "font-family": "var(--font-sans)",
      "font-size": 12,
      fill:
        best.rpki.status === "valid"
          ? "var(--accent-best)"
          : best.rpki.status === "invalid"
            ? "var(--accent-invalid)"
            : "var(--text-muted)",
    });
    badge.textContent = `RPKI: ${t(`rpki.${best.rpki.status}`)}`;
  }

  const container = document.getElementById("graph");
  container.replaceChildren(svg);
  container.setAttribute(
    "aria-label",
    `${t("result.best_path")}: ${best.as_path.join(" → ")}`,
  );
  document.getElementById("graph-panel").hidden = false;
}

async function start() {
  const fromPath = window.location.pathname.split("/")[1];
  if (LOCALES.includes(fromPath)) state.locale = fromPath;
  document.documentElement.lang = CATALOGUE_FILE[state.locale] ?? "en";

  await loadMessages(state.locale);
  applyTranslations();
  await loadRouters();
  loadVersion();

  document.getElementById("query-form").addEventListener("submit", onSubmit);
}

start();
