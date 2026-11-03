(function () {
  try {
    const storedTheme = localStorage.getItem("agenthub.theme");
    if (storedTheme === "dark" || storedTheme === "light") {
      document.documentElement.setAttribute("data-theme", storedTheme);
    } else {
      document.documentElement.removeAttribute("data-theme");
    }
  } catch {
    // Ignore storage access errors before mount.
  }

  function messageFrom(error) {
    if (!error) return "前端入口没有完成挂载";
    if (typeof error === "string") return error;
    if (error.message) return error.message;
    if (error.reason) return messageFrom(error.reason);
    if (error.type) return error.type;
    return String(error);
  }

  window.__AGENTHUB_BOOT__ = {
    mounted: false,
    showError(error) {
      const root = document.getElementById("root");
      if (!root || this.mounted) return;
      root.innerHTML = "<main class=\"fatal-screen\"><section class=\"panel wide-panel\"><header><strong>AgentHub 界面载入失败</strong></header><div class=\"form-error\"></div></section></main>";
      const target = root.querySelector(".form-error");
      if (target) target.textContent = messageFrom(error);
    }
  };

  window.addEventListener("error", (event) => {
    window.__AGENTHUB_BOOT__?.showError(event.error || event.message || event);
  });
  window.addEventListener("unhandledrejection", (event) => {
    window.__AGENTHUB_BOOT__?.showError(event.reason || event);
  });
  window.setTimeout(() => {
    if (!window.__AGENTHUB_BOOT__?.mounted) {
      window.__AGENTHUB_BOOT__?.showError("前端入口没有完成挂载");
    }
  }, 3000);
})();
