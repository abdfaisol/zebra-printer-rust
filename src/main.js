import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

let currentConfig = null;

// Safe invoke wrapper for Browser Preview / Tauri Native
async function safeInvoke(cmd, args = {}) {
  try {
    if (window.__TAURI_INTERNALS__ || window.__TAURI__) {
      return await invoke(cmd, args);
    }
  } catch (e) {
    console.warn("Tauri invoke error:", e);
  }
  // Mock fallback for browser preview
  if (cmd === "get_config") {
    return {
      api_base_url: "https://dev-eam-api.midsuit.com/api/v1/",
      api_token: "d85045f7-b475-4ffe-8b75-3a1a8757033b",
      path_listen_job: "printers/listen-print-job",
      path_update_job: "printers/update-print-job",
      path_printers: "printers",
      polling_enabled: true,
      polling_interval_ms: 1000,
      auto_sync_printers: true,
      run_at_startup: false,
      tcp_enabled: true,
      tcp_host: "127.0.0.1",
      tcp_port: 9100,
      printers: [
        {
          id: "bb91653d-d151-4e72-9630-88ce1adf3d87",
          address: "tcp://127.0.0.1:9100",
          alias_name: "FAISOL",
          selected_for_sync: true,
          online: false,
        },
      ],
    };
  }
  return null;
}
// Modern Toast Notification Popup (Pengganti alert bawaan)
function showToast(message, type = "success") {
  const container = document.getElementById("toast-container");
  if (!container) return;

  const toast = document.createElement("div");
  const isSuccess = type === "success";
  
  toast.className = `pointer-events-auto flex items-center space-x-2.5 px-4 py-3 rounded-xl shadow-lg border text-sm font-medium transition-all duration-300 transform translate-y-2 opacity-0 ${
    isSuccess
      ? "bg-emerald-900 border-emerald-700 text-white shadow-emerald-950/20"
      : "bg-rose-900 border-rose-700 text-white shadow-rose-950/20"
  }`;

  toast.innerHTML = `
    <span class="text-base">${isSuccess ? "✓" : "⚠"}</span>
    <span>${message}</span>
  `;

  container.appendChild(toast);

  // Animate in
  setTimeout(() => {
    toast.classList.remove("translate-y-2", "opacity-0");
    toast.classList.add("translate-y-0", "opacity-100");
  }, 10);

  // Animate out & remove
  setTimeout(() => {
    toast.classList.add("translate-y-2", "opacity-0");
    setTimeout(() => toast.remove(), 300);
  }, 3200);
}

const tabBtns = {
  config: document.getElementById("tab-btn-config"),
  printers: document.getElementById("tab-btn-printers"),
  logs: document.getElementById("tab-btn-logs"),
};

const tabContents = {
  config: document.getElementById("tab-content-config"),
  printers: document.getElementById("tab-content-printers"),
  logs: document.getElementById("tab-content-logs"),
};

function switchTab(name) {
  Object.keys(tabBtns).forEach((k) => {
    if (k === name) {
      tabBtns[k].className = "py-3 border-b-2 border-amber-500 text-amber-600 font-semibold focus:outline-hidden";
      tabContents[k].classList.remove("hidden");
    } else {
      tabBtns[k].className = "py-3 border-b-2 border-transparent text-slate-500 hover:text-slate-700 focus:outline-hidden";
      tabContents[k].classList.add("hidden");
    }
  });
}

tabBtns.config.addEventListener("click", () => switchTab("config"));
tabBtns.printers.addEventListener("click", () => switchTab("printers"));
tabBtns.logs.addEventListener("click", () => switchTab("logs"));

// Logs
const logConsole = document.getElementById("log-console");
function appendLog(msg) {
  const p = document.createElement("p");
  const time = new Date().toLocaleTimeString();
  p.textContent = `[${time}] ${msg}`;
  logConsole.appendChild(p);
  logConsole.scrollTop = logConsole.scrollHeight;
}

document.getElementById("btn-clear-logs").addEventListener("click", () => {
  logConsole.innerHTML = '<p class="text-slate-500">[System] Layar dibersihkan.</p>';
});

document.getElementById("btn-open-logs").addEventListener("click", async () => {
  await invoke("open_log_folder");
});

// Load Config
async function loadConfig() {
  try {
    currentConfig = await safeInvoke("get_config");
    if (!currentConfig) return;
    document.getElementById("cfg-base-url").value = currentConfig.api_base_url || "";
    document.getElementById("cfg-api-token").value = currentConfig.api_token || "";
    document.getElementById("cfg-path-listen").value = currentConfig.path_listen_job || "";
    document.getElementById("cfg-path-update").value = currentConfig.path_update_job || "";
    document.getElementById("cfg-path-printers").value = currentConfig.path_printers || "printers";
    document.getElementById("cfg-tcp-enabled").checked = currentConfig.tcp_enabled || false;
    document.getElementById("cfg-tcp-host").value = currentConfig.tcp_host || "127.0.0.1";
    document.getElementById("cfg-tcp-port").value = currentConfig.tcp_port || 9100;
    document.getElementById("cfg-interval-ms").value = currentConfig.polling_interval_ms || 1000;
    document.getElementById("cfg-polling-enabled").checked = currentConfig.polling_enabled !== false;
    document.getElementById("cfg-auto-sync").checked = currentConfig.auto_sync_printers !== false;
    document.getElementById("cfg-run-startup").checked = currentConfig.run_at_startup || false;
    renderPrintersTable(currentConfig.printers || []);
  } catch (e) {
    appendLog(`Gagal memuat konfigurasi: ${e}`);
  }
}

// Reset Default
document.getElementById("btn-reset-default").addEventListener("click", () => {
  document.getElementById("cfg-base-url").value = "https://dev-eam-api.midsuit.com/api/v1/";
  document.getElementById("cfg-path-listen").value = "printers/listen-print-job";
  document.getElementById("cfg-path-update").value = "printers/update-print-job";
  document.getElementById("cfg-path-printers").value = "printers";
  document.getElementById("cfg-tcp-enabled").checked = true;
  document.getElementById("cfg-tcp-host").value = "127.0.0.1";
  document.getElementById("cfg-tcp-port").value = 9100;
  document.getElementById("cfg-interval-ms").value = 1000;
  document.getElementById("cfg-polling-enabled").checked = true;
  document.getElementById("cfg-auto-sync").checked = true;
  document.getElementById("cfg-run-startup").checked = false;
  appendLog("Form pengaturan dikembalikan ke nilai default.");
});

// Save Config
document.getElementById("btn-save-config").addEventListener("click", async () => {
  if (!currentConfig) return;
  currentConfig.api_base_url = document.getElementById("cfg-base-url").value.trim();
  currentConfig.api_token = document.getElementById("cfg-api-token").value.trim();
  currentConfig.path_listen_job = document.getElementById("cfg-path-listen").value.trim();
  currentConfig.path_update_job = document.getElementById("cfg-path-update").value.trim();
  currentConfig.path_printers = document.getElementById("cfg-path-printers").value.trim();
  currentConfig.tcp_enabled = document.getElementById("cfg-tcp-enabled").checked;
  currentConfig.tcp_host = document.getElementById("cfg-tcp-host").value.trim();
  currentConfig.tcp_port = parseInt(document.getElementById("cfg-tcp-port").value, 10) || 9100;
  currentConfig.polling_interval_ms = parseInt(document.getElementById("cfg-interval-ms").value, 10) || 1000;
  currentConfig.polling_enabled = document.getElementById("cfg-polling-enabled").checked;
  currentConfig.auto_sync_printers = document.getElementById("cfg-auto-sync").checked;
  currentConfig.run_at_startup = document.getElementById("cfg-run-startup").checked;

  try {
    await invoke("save_config", { newCfg: currentConfig });
    appendLog("Pengaturan server & koneksi berhasil disimpan.");
    showToast("Pengaturan berhasil disimpan!", "success");
  } catch (e) {
    appendLog(`Gagal menyimpan: ${e}`);
    showToast(`Gagal menyimpan: ${e}`, "error");
  }
});

// Printers Table
function renderPrintersTable(printers) {
  const tbody = document.getElementById("printer-table-body");
  tbody.innerHTML = "";

  let onlineCount = 0;
  printers.forEach((p, idx) => {
    if (p.online) onlineCount++;
    const tr = document.createElement("tr");
    tr.className = "hover:bg-slate-50";

    tr.innerHTML = `
      <td class="py-3 px-4 text-center">
        <input type="checkbox" ${p.selected_for_sync ? "checked" : ""} data-idx="${idx}" class="sync-chk w-4 h-4 text-amber-500 rounded-sm" />
      </td>
      <td class="py-3 px-4 font-mono text-xs text-slate-600">${p.address}</td>
      <td class="py-3 px-4">
        <input type="text" value="${p.alias_name || ""}" data-idx="${idx}" class="alias-input w-full px-2 py-1 text-sm border border-slate-200 rounded-md" />
      </td>
      <td class="py-3 px-4 text-center">
        <span class="inline-flex px-2.5 py-0.5 rounded-full text-xs font-bold ${
          p.online ? "bg-emerald-100 text-emerald-700" : "bg-rose-100 text-rose-700"
        }">
          ${p.online ? "ONLINE" : "OFFLINE"}
        </span>
      </td>
    `;
    tbody.appendChild(tr);
  });

  const footerCount = document.getElementById("footer-printer-count");
  if (onlineCount > 0) {
    footerCount.textContent = `${onlineCount} printer terhubung`;
    footerCount.className = "text-emerald-600 font-medium";
  } else {
    footerCount.textContent = "Tidak ada printer terhubung";
    footerCount.className = "text-slate-500";
  }

  // Bind change events
  document.querySelectorAll(".sync-chk").forEach((el) => {
    el.addEventListener("change", (e) => {
      const i = e.target.dataset.idx;
      currentConfig.printers[i].selected_for_sync = e.target.checked;
    });
  });

  document.querySelectorAll(".alias-input").forEach((el) => {
    el.addEventListener("input", (e) => {
      const i = e.target.dataset.idx;
      currentConfig.printers[i].alias_name = e.target.value;
    });
  });

  document.querySelectorAll(".test-btn").forEach((el) => {
    el.addEventListener("click", async (e) => {
      const addr = e.target.dataset.addr;
      try {
        const res = await invoke("test_print", { address: addr });
        appendLog(res);
      } catch (err) {
        appendLog(`Gagal tes print: ${err}`);
      }
    });
  });
}

// Scan Button
document.getElementById("btn-scan").addEventListener("click", async () => {
  appendLog("Memulai pemindaian printer...");
  try {
    const discovered = await invoke("scan_printers");
    appendLog(`Pemindaian selesai: ditemukan ${discovered.length} printer.`);
    discovered.forEach((d) => {
      const existing = currentConfig.printers.find((p) => p.address === d.address);
      if (!existing) {
        currentConfig.printers.push({
          id: null,
          address: d.address,
          alias_name: d.name,
          selected_for_sync: true,
          online: d.is_online,
        });
      } else {
        existing.online = d.is_online;
      }
    });
    renderPrintersTable(currentConfig.printers);
  } catch (err) {
    appendLog(`Error scan: ${err}`);
  }
});

// Save Printers (Simpan lokal sekaligus picu sinkron ke server)
document.getElementById("btn-save-printers").addEventListener("click", async () => {
  if (!currentConfig) return;
  try {
    await invoke("save_config", { newCfg: currentConfig });
    appendLog("Daftar printer disimpan. Memulai sinkronisasi ke server...");
    showToast("Daftar printer disimpan, menyinkronkan...", "success");

    // Langsung picu sinkronisasi ke server
    try {
      const res = await invoke("sync_all_printers_to_server");
      appendLog(res);
      showToast("Sinkronisasi ke server sukses!", "success");
    } catch (syncErr) {
      appendLog(`Catatan sinkron server: ${syncErr}`);
    }
  } catch (err) {
    appendLog(`Gagal simpan printer: ${err}`);
    showToast(`Gagal simpan printer: ${err}`, "error");
  }
});

// Tombol Sinkron ke Server manual
document.getElementById("btn-sync-server").addEventListener("click", async () => {
  if (!currentConfig) return;
  try {
    await invoke("save_config", { newCfg: currentConfig });
    appendLog("Memulai sinkronisasi manual ke server...");
    showToast("Menyinkronkan ke server...", "success");
    const res = await invoke("sync_all_printers_to_server");
    appendLog(res);
    showToast("Sinkronisasi selesai!", "success");
  } catch (err) {
    appendLog(`Gagal sinkron server: ${err}`);
    showToast(`Gagal: ${err}`, "error");
  }
});

// Tauri Event Listeners
listen("log-message", (event) => {
  appendLog(event.payload);
});

listen("printers-updated", (event) => {
  if (currentConfig) {
    currentConfig.printers = event.payload;
    renderPrintersTable(currentConfig.printers);
  }
});

// Init
window.addEventListener("DOMContentLoaded", () => {
  loadConfig();
});
