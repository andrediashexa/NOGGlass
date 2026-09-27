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
    const ver = info.version && info.version !== "dev" ? info.version : "1.0.0";
    const versionEl = document.getElementById("version");
    if (versionEl) {
      versionEl.textContent = ver;
    }
    const buildMeta = document.getElementById("build-meta");
    if (buildMeta) {
      if (info.commit && info.commit !== "unknown") {
        let meta = `· ${t("about.commit")} ${info.commit}`;
        if (info.built_at && info.built_at !== "unknown") {
          meta += ` · ${t("about.built")} ${info.built_at}`;
        }
        buildMeta.textContent = meta;
        buildMeta.hidden = false;
      } else {
        buildMeta.textContent = "";
        buildMeta.hidden = true;
      }
    }
    const link = document.getElementById("project-link") || document.getElementById("version-link");
    if (link) {
      link.href = "https://github.com/andrediashexa/looking-glass";
    }
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
    "sessions-panel",
  ]) {
    document.getElementById(id).hidden = true;
  }
}

function onClear(event) {
  event?.preventDefault();
  const targetInput = document.getElementById("target");
  if (targetInput) {
    targetInput.value = "";
    targetInput.focus();
  }
  const errorBox = document.getElementById("error");
  if (errorBox) {
    errorBox.textContent = "";
    errorBox.hidden = true;
  }
  const rawPre = document.getElementById("raw");
  if (rawPre) {
    rawPre.textContent = "";
  }
  const graph = document.getElementById("graph");
  if (graph) {
    graph.replaceChildren();
  }
  for (const tableId of ["paths-table", "hops-table", "sessions-table"]) {
    const tbody = document.querySelector(`#${tableId} tbody`);
    if (tbody) tbody.replaceChildren();
  }
  hidePanels();
  const url = new URL(window.location.href);
  url.searchParams.delete("target");
  url.searchParams.delete("type");
  window.history.replaceState(null, "", url);
}
/**
 * Reflects the query in the address bar, so copying the URL shares what is on
 * screen. `replaceState` rather than `pushState`: a visitor trying three
 * prefixes wants Back to leave the page, not to walk their own attempts.
 */
function rememberInUrl(payload) {
  const url = new URL(window.location.href);
  url.searchParams.set("router", payload.router);
  url.searchParams.set("type", payload.type);
  url.searchParams.set("target", payload.target);
  window.history.replaceState(null, "", url);
}

/**
 * Runs the query a link asked for.
 *
 * Everything here is untrusted: a URL is as much visitor input as the form is.
 * The router and query type are checked against what this instance offers, and
 * the target goes to the server, which validates it exactly as it validates
 * typed input.
 */
function applyQueryFromUrl() {
  const params = new URLSearchParams(window.location.search);
  const target = params.get("target");
  if (!target) return false;

  const routerField = document.getElementById("router");
  const wanted = params.get("router");
  if (wanted && state.routers.some((router) => router.id === wanted)) {
    routerField.value = wanted;
    onRouterChange();
  }

  const queryField = document.getElementById("query-type");
  const type = params.get("type");
  if (type && [...queryField.options].some((option) => option.value === type)) {
    queryField.value = type;
  }

  document.getElementById("target").value = target;
  return true;
}

let pendingPayload = null;

async function fetchCaptcha() {
  try {
    const res = await fetch("/api/captcha");
    if (!res.ok) throw new Error("failed to fetch captcha");
    const data = await res.json();
    document.getElementById("captcha-id").value = data.captcha_id;
    document.getElementById("captcha-svg-container").innerHTML = data.captcha_svg;
    const input = document.getElementById("captcha-input");
    input.value = "";
    input.focus();
    document.getElementById("captcha-error").hidden = true;
  } catch (err) {
    document.getElementById("captcha-error").textContent = t("error.unreachable");
    document.getElementById("captcha-error").hidden = false;
  }
}

function showCaptchaModal(payload) {
  pendingPayload = payload;
  fetchCaptcha();
  const modal = document.getElementById("captcha-modal");
  if (modal && !modal.open) {
    modal.showModal();
  }
}

async function onCaptchaSubmit(event) {
  event?.preventDefault();
  const captchaId = document.getElementById("captcha-id").value;
  const captchaCode = document.getElementById("captcha-input").value.trim().toUpperCase();

  if (!captchaCode) return;

  const payload = {
    ...pendingPayload,
    captcha_id: captchaId,
    captcha_code: captchaCode,
  };

  const submitBtn = document.getElementById("captcha-submit");
  submitBtn.disabled = true;

  try {
    const response = await fetch("/api/query", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload),
    });
    const body = await response.json();
    if (!response.ok) {
      if (body.code === "captcha_invalid" || body.code === "captcha_required") {
        document.getElementById("captcha-error").textContent =
          state.messages[`error.${body.code}`] ?? body.message;
        document.getElementById("captcha-error").hidden = false;
        fetchCaptcha();
        return;
      }
      document.getElementById("captcha-modal").close();
      showError(body.code, body.message);
      return;
    }

    document.getElementById("captcha-modal").close();
    render(body, payload.target);
  } catch (error) {
    document.getElementById("captcha-modal").close();
    showError("unreachable", String(error));
  } finally {
    submitBtn.disabled = false;
  }
}
async function onSubmit(event) {
  event?.preventDefault();
  hidePanels();

  let answer = null;

  const button = document.getElementById("submit");
  const label = button.querySelector("span");
  button.disabled = true;
  label.textContent = t("form.running");

  const payload = {
    router: document.getElementById("router").value,
    type: document.getElementById("query-type").value,
    target: document.getElementById("target").value,
  };
  rememberInUrl(payload);

  try {
    const response = await fetch("/api/query", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload),
    });
    const body = await response.json();
    if (!response.ok) {
      if (body.code === "captcha_required") {
        showCaptchaModal(payload);
        return;
      }
      showError(body.code, body.message);
      return;
    }
    answer = body;
  } catch (error) {
    showError("unreachable", String(error));
  } finally {
    button.disabled = false;
    label.textContent = t("form.submit");
  }

  // Drawing happens outside the catch above, which is about reaching the
  // router. A bug in rendering used to land there and tell the visitor the
  // router could not be reached — while its answer sat on the screen
  // underneath, looking stale and not being.
  if (answer) {
    render(answer, payload.target);
  }
}

function render(response, target) {
  const meta = document.getElementById("result-meta");
  meta.textContent = `${t("result.command")}: ${response.command} · ${t("result.duration")} ${response.duration_ms} ms`;

  if (response.kind === "bgp_route") {
    renderBgp(response.result, target);
    renderGlobalView(response.agreement, response.global);
  } else if (response.kind === "raw") {
    renderRaw(response.result?.raw_output ?? response.output, response.truncated);
  } else if (response.kind === "bgp_summary") {
    renderRaw(response.result.raw_output, false);
    renderSessions(response.result);
  } else if (response.kind === "traceroute") {
    renderRaw(response.result.raw_output, false);
    renderHops(response.result.hops);
  } else {
    renderRaw(response.result.raw_output, false);
  }
}

function escapeHtml(text) {
  return String(text)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

function highlightBgpRaw(raw, truncated) {
  if (!raw) return "";

  let html = "";
  const bestBadgeText = t("badge.best") ?? "Rota Ativa";

  // 1. Huawei detailed output (contains "BGP routing table entry information of")
  if (/BGP routing table entry information of/i.test(raw)) {
    const parts = raw.split(/(?=BGP routing table entry information of)/i);
    for (const part of parts) {
      const isBest =
        /\b(?:best,\s*select|valid,\s*(?:external|internal),\s*best)\b/i.test(part) &&
        !/not preferred/i.test(part);

      let escaped = escapeHtml(part);
      if (isBest) {
        escaped = escaped.replace(
          /(BGP routing table entry information of[^\n:]*:?)/i,
          `$1 <span class="raw-best-badge">★ ${escapeHtml(bestBadgeText)}</span>`
        );
        html += `<span class="raw-best-route">${escaped}</span>`;
      } else {
        html += escaped;
      }
    }
  }
  // 2. Cisco / generic detailed output with "Paths: (" or "Routing entry for"
  else if (/Routing entry for|Paths:\s*\(/i.test(raw) && /\b(?:best|best\s*#\d+)\b/i.test(raw)) {
    const lines = raw.split("\n");
    let inBestBlock = false;
    let currentBlock = [];

    for (const line of lines) {
      if (/^\s*(?:Path\s+\d+|[0-9]+(?:\s+[0-9]+)*\s*$)/i.test(line)) {
        if (currentBlock.length) {
          const blockText = currentBlock.join("\n");
          if (inBestBlock) {
            html += `<span class="raw-best-route">${escapeHtml(blockText)}</span>\n`;
          } else {
            html += `${escapeHtml(blockText)}\n`;
          }
          currentBlock = [];
          inBestBlock = false;
        }
      }
      if (/\b(?:valid,\s*(?:external|internal),\s*best|\(best\))\b/i.test(line)) {
        inBestBlock = true;
      }
      currentBlock.push(line);
    }
    if (currentBlock.length) {
      const blockText = currentBlock.join("\n");
      if (inBestBlock) {
        html += `<span class="raw-best-route">${escapeHtml(blockText)}</span>`;
      } else {
        html += escapeHtml(blockText);
      }
    }
  }
  // 3. Tabular formats (Cisco, Huawei tabular, BIRD, MikroTik)
  else {
    const lines = raw.split("\n");
    const resultLines = [];

    for (const line of lines) {
      const isCiscoBest = /^\s*\*>\S*/.test(line);
      const isMikrotikBest = /^\s*\d+\s+DA[bv]/.test(line) || /^\s*DA[bv]/.test(line);
      const isBirdBest = /\[\*BGP/i.test(line) || /\*\s+\(\d+\)/.test(line);

      if (isCiscoBest || isMikrotikBest || isBirdBest) {
        resultLines.push(`<span class="raw-best-route">${escapeHtml(line)}</span>`);
      } else {
        resultLines.push(escapeHtml(line));
      }
    }
    html = resultLines.join("\n");
  }

  if (truncated) {
    html += `\n\n<span class="meta">${escapeHtml(t("result.truncated"))}</span>`;
  }
  return html;
}

function renderRaw(output, truncated, isBgp = false) {
  const panel = document.getElementById("raw-panel");
  const pre = document.getElementById("raw");
  if (!output) {
    pre.textContent = "";
    panel.hidden = false;
    return;
  }

  if (isBgp) {
    pre.innerHTML = highlightBgpRaw(output, truncated);
  } else {
    pre.textContent = output;
    if (truncated) {
      pre.textContent += `\n\n${t("result.truncated")}`;
    }
  }
  panel.hidden = false;
}

function renderBgp(result, queryTarget) {
  renderRaw(result.raw_output, result.truncated, true);

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
  // Não convém gerar gráfico quando for consulta por ASN ou quando houver
  // dezenas/centenas de rotas, pois o diagrama SVG fica ilegível e sobrecarregado.
  const isAsnQuery = /^as\d+/i.test(queryTarget?.trim() ?? "");
  const tooManyPaths = result.paths.length > 10;

  if (isAsnQuery || tooManyPaths) {
    document.getElementById("graph-panel").hidden = true;
    document.getElementById("graph").replaceChildren();
  } else {
    try {
      renderGraph(result.paths);
    } catch (err) {
      console.error("Failed to render AS path graph:", err);
    }
  }
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
/**
 * Renders the session table. A session that is not established is coloured,
 * because finding those is the whole reason this query exists.
 */
function renderSessions(summary) {
  const meta = document.getElementById("sessions-meta");
  const parts = [];
  if (summary.router_id) parts.push(`${t("summary.router_id")}: ${summary.router_id}`);
  if (summary.local_as != null) parts.push(`${t("summary.local_as")}: AS${summary.local_as}`);
  meta.textContent = parts.join(" · ");

  const body = document.querySelector("#sessions-table tbody");
  body.replaceChildren();

  for (const peer of summary.peers) {
    const established = peer.state.toLowerCase() === "established";
    const row = document.createElement("tr");
    row.dataset.established = String(established);

    const cells = [
      peer.peer_ip,
      `AS${peer.peer_as}`,
      established ? t("summary.established") : peer.state,
      peer.uptime || t("result.unknown"),
      established ? String(peer.prefixes_received) : t("result.unknown"),
    ];

    for (const value of cells) {
      const cell = document.createElement("td");
      cell.textContent = value;
      if (value === t("result.unknown")) cell.className = "unknown";
      row.appendChild(cell);
    }
    body.appendChild(row);
  }

  document.getElementById("sessions-panel").hidden = false;
}

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
        cell.className = "cell-communities";
        if (!path.communities.length) {
          cell.textContent = t("result.unknown");
          cell.classList.add("unknown");
          return;
        }
        const wrap = document.createElement("div");
        wrap.className = "communities-wrap";
        for (const community of path.communities) {
          const chip = document.createElement("span");
          chip.className = "community";
          chip.textContent = community.raw;
          if (community.name) chip.title = community.name;
          wrap.appendChild(chip);
        }
        cell.appendChild(wrap);
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
  if (!paths || !paths.length) return;

  // 1. Sanitize paths: collapse consecutive AS-prepends for clean topological node representation
  const cleanPaths = paths.map((p) => {
    const rawAsPath = Array.isArray(p.as_path) ? p.as_path : [];
    const deduped = rawAsPath.filter(
      (asn, idx, arr) => idx === 0 || String(asn) !== String(arr[idx - 1]),
    );
    return {
      ...p,
      hops: ["local", ...deduped.map(String)],
    };
  });

  // 2. Topological rank assignment (longest-path DAG relaxation from the local router)
  const ranks = new Map();
  ranks.set("local", 0);

  for (const path of cleanPaths) {
    for (const asn of path.hops) {
      if (!ranks.has(asn)) ranks.set(asn, 0);
    }
  }

  const allHops = cleanPaths.map((p) => p.hops);
  const numNodes = ranks.size;
  for (let iter = 0; iter < numNodes; iter++) {
    let changed = false;
    for (const hops of allHops) {
      for (let i = 0; i < hops.length - 1; i++) {
        const u = hops[i];
        const v = hops[i + 1];
        const requiredRank = ranks.get(u) + 1;
        if (ranks.get(v) < requiredRank) {
          ranks.set(v, requiredRank);
          changed = true;
        }
      }
    }
    if (!changed) break;
  }

  const maxRank = Math.max(...ranks.values(), 1);
  const originAsns = new Set(
    cleanPaths.map((p) => p.hops[p.hops.length - 1]).filter(Boolean),
  );

  // A terminal origin has no outgoing edges in any path; align to maxRank
  const hasOutgoing = new Set();
  for (const hops of allHops) {
    for (let i = 0; i < hops.length - 1; i++) {
      hasOutgoing.add(hops[i]);
    }
  }

  for (const origin of originAsns) {
    if (!hasOutgoing.has(origin)) {
      ranks.set(origin, maxRank);
    }
  }

  // 3. Group nodes by contiguous column ranks
  const placed = new Map();
  for (const [asn, rank] of ranks) {
    if (asn === "local") continue;
    if (!placed.has(rank)) placed.set(rank, []);
    placed.get(rank).push(asn);
  }

  const columns = [["local"]];
  const sortedRanks = [...placed.keys()].sort((a, b) => a - b);
  for (const rank of sortedRanks) {
    columns.push(placed.get(rank));
  }

  const nodes = new Map();
  for (const [colIndex, members] of columns.entries()) {
    for (const asn of members) {
      nodes.set(asn, { asn, column: colIndex });
    }
  }

  // 4. Vertical layout (Y-coordinates) using path centroids
  const pathIndices = new Map();
  cleanPaths.forEach((path, pIdx) => {
    for (const asn of path.hops) {
      if (!pathIndices.has(asn)) pathIndices.set(asn, []);
      pathIndices.get(asn).push(pIdx);
    }
  });

  const numPaths = Math.max(cleanPaths.length, 1);
  const maxColumnMembers = Math.max(...columns.map((c) => c.length), 1);
  const columnWidth = 190;
  const rowHeight = 96;
  const radius = 34;
  const height = Math.max(
    Math.max(numPaths, maxColumnMembers) * rowHeight + 80,
    240,
  );
  const centerY = height / 2;

  for (const [colIndex, members] of columns.entries()) {
    if (members.length === 1 && members[0] === "local") {
      const node = nodes.get("local");
      node.x = 60;
      node.y = centerY;
      continue;
    }

    // Sort members in column by their average path index (top to bottom)
    members.sort((a, b) => {
      const aIndices = pathIndices.get(a) ?? [0];
      const bIndices = pathIndices.get(b) ?? [0];
      const aAvg = aIndices.reduce((sum, v) => sum + v, 0) / aIndices.length;
      const bAvg = bIndices.reduce((sum, v) => sum + v, 0) / bIndices.length;
      if (Math.abs(aAvg - bAvg) > 0.001) return aAvg - bAvg;
      const aBest = cleanPaths[0]?.hops.includes(a) ? 0 : 1;
      const bBest = cleanPaths[0]?.hops.includes(b) ? 0 : 1;
      if (aBest !== bBest) return aBest - bBest;
      return String(a).localeCompare(String(b));
    });

    if (members.length === 1) {
      const asn = members[0];
      const indices = pathIndices.get(asn) ?? [0];
      const isSharedAll = indices.length === numPaths;
      const node = nodes.get(asn);
      node.x = 60 + colIndex * columnWidth;
      if (isSharedAll) {
        node.y = centerY;
      } else {
        const avg = indices.reduce((sum, v) => sum + v, 0) / indices.length;
        const offset = (avg - (numPaths - 1) / 2) * rowHeight;
        node.y = Math.max(radius + 25, Math.min(height - radius - 25, centerY + offset));
      }
    } else {
      const minDistance = radius * 2 + 24;
      const totalSpan = (members.length - 1) * minDistance;
      const startY = Math.max(radius + 25, centerY - totalSpan / 2);

      members.forEach((asn, idx) => {
        const node = nodes.get(asn);
        node.x = 60 + colIndex * columnWidth;
        node.y = startY + idx * minDistance;
      });
    }
  }

  const width = columns.length * columnWidth + 80;

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

  // 5. Edges: draw alternative/backup paths first, best path last so it renders cleanly on top
  const sortedPaths = [...cleanPaths].sort(
    (a, b) => (a.is_best ? 1 : 0) - (b.is_best ? 1 : 0),
  );

  for (const path of sortedPaths) {
    for (let i = 0; i < path.hops.length - 1; i += 1) {
      const from = nodes.get(path.hops[i]);
      const to = nodes.get(path.hops[i + 1]);
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

  // 6. Nodes (Circles & Labels)
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
      fill: "var(--graph-node-bg)",
      stroke: colour,
      "stroke-width": 2.5,
      filter: "drop-shadow(0 2px 4px var(--graph-shadow))",
    });

    const label = draw("text", {
      x: node.x,
      y: node.y + 5,
      "text-anchor": "middle",
      fill: "var(--graph-node-text)",
      "font-family": "var(--font-mono)",
      "font-size": 13,
      "font-weight": "600",
    });
    label.textContent = isLocal ? t("form.router") : `AS${node.asn}`;
  }

  // 7. RPKI state of the best path, next to its origin
  const best = paths.find((p) => p.is_best) ?? paths[0];
  const cleanBest = cleanPaths.find((p) => p.is_best) ?? cleanPaths[0];
  const originAsn = cleanBest.hops[cleanBest.hops.length - 1];
  const originNode = nodes.get(originAsn);
  if (originNode && best.rpki?.status) {
    const badge = draw("text", {
      x: originNode.x,
      y: originNode.y + radius + 20,
      "text-anchor": "middle",
      "font-family": "var(--font-sans)",
      "font-size": 12,
      "font-weight": "600",
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
  document.getElementById("clear-btn")?.addEventListener("click", onClear);
  document.getElementById("captcha-form")?.addEventListener("submit", onCaptchaSubmit);
  document.getElementById("captcha-refresh")?.addEventListener("click", fetchCaptcha);

  // A link that carries a query runs it, so a result can be shared.
  if (applyQueryFromUrl()) {
    await onSubmit();
  }
}

start();
