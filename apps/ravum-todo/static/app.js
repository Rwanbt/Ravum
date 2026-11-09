// Ravum TODO — client minimal, sans framework, sans étape de build.
//
// Contrat : le serveur expose /api/schema. Si `contract_version` diffère de ce que
// ce client sait lire, toutes les mutations sont désactivées. Il n'existe aucune
// compatibilité ascendante implicite.

const CLIENT_CONTRACT_VERSION = 1;

const banner = document.getElementById("banner");
const errorBox = document.getElementById("error");
const list = document.getElementById("tasks");
const form = document.getElementById("new-task");
const tokenInput = document.getElementById("token");
const titleInput = document.getElementById("title");
const dueInput = document.getElementById("due");
const refreshButton = document.getElementById("refresh");

let compatible = false;

function showError(message) {
  errorBox.textContent = message;
}

function clearError() {
  errorBox.textContent = "";
}

function showBanner(kind, text) {
  banner.className = `banner ${kind}`;
  banner.textContent = text;
  banner.style.display = "block";
}

function authHeaders() {
  const token = tokenInput.value.trim();
  if (!token) {
    throw new Error("saisissez un jeton pour modifier les tâches");
  }
  return {
    "Content-Type": "application/json",
    Authorization: `Bearer ${token}`,
  };
}

async function api(method, path, body) {
  // Every mutation carries the bearer token, including DELETE (which has no body).
  // Before MVP-0b, only requests with a body sent Authorization, so delete always
  // answered 401 — a real UI bug that HTTP-only tests could not see.
  const headers = method === "GET" ? {} : authHeaders();
  const response = await fetch(path, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (response.status === 204) {
    return null;
  }
  const payload = await response.json().catch(() => null);
  if (!response.ok) {
    const detail = payload && payload.error ? payload.error : { message: `HTTP ${response.status}` };
    const violations = (detail.details || [])
      .map((d) => (d.message ? d.message : JSON.stringify(d)))
      .join("; ");
    throw new Error(violations ? `${detail.message}: ${violations}` : detail.message);
  }
  return payload;
}

async function checkContract() {
  const response = await fetch("/api/schema");
  const contract = await response.json();
  if (contract.contract_version !== CLIENT_CONTRACT_VERSION) {
    compatible = false;
    showBanner(
      "warn",
      `Client incompatible : contrat serveur v${contract.contract_version}, client v${CLIENT_CONTRACT_VERSION}. Mutations désactivées.`,
    );
    form.querySelector("button").disabled = true;
    return;
  }
  compatible = true;
  showBanner("ok", `Contrat v${contract.contract_version} vérifié.`);
}

function render(tasks) {
  list.replaceChildren();
  for (const task of tasks) {
    const item = document.createElement("li");
    item.dataset.taskId = String(task.id);
    if (task.done) {
      item.classList.add("done");
    }

    const title = document.createElement("span");
    title.className = "title";
    title.textContent = task.title;
    item.append(title);

    if (task.due_date) {
      const due = document.createElement("span");
      due.className = "due";
      due.textContent = task.due_date;
      item.append(due);
    }

    const rename = document.createElement("button");
    rename.type = "button";
    rename.textContent = "Modifier";
    rename.disabled = !compatible;
    rename.addEventListener("click", () => startRename(item, task));
    item.append(rename);

    const toggle = document.createElement("button");
    toggle.type = "button";
    toggle.textContent = task.done ? "Rouvrir" : "Terminer";
    toggle.disabled = !compatible;
    toggle.addEventListener("click", () => mutate(() => api("PATCH", `/api/tasks/${task.id}`, { done: !task.done })));
    item.append(toggle);

    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "Supprimer";
    remove.disabled = !compatible;
    remove.addEventListener("click", () => mutate(() => api("DELETE", `/api/tasks/${task.id}`)));
    item.append(remove);

    list.append(item);
  }
  if (tasks.length === 0) {
    const empty = document.createElement("li");
    empty.className = "muted";
    empty.textContent = "aucune tâche";
    list.append(empty);
  }
}

// Inline title editor: the API supports PATCH {title} since M4, the UI exposes it here
// so renaming is reachable without devtools. The whole list is re-rendered afterwards,
// like every other mutation.
function startRename(item, task) {
  if (item.querySelector(".edit-title")) {
    return;
  }
  const title = item.querySelector(".title");
  if (!title) {
    return;
  }

  const input = document.createElement("input");
  input.type = "text";
  input.className = "edit-title";
  input.value = task.title;
  input.maxLength = 200;
  input.setAttribute("aria-label", "Nouveau titre");

  const save = document.createElement("button");
  save.type = "button";
  save.textContent = "Enregistrer";
  save.disabled = !compatible;

  const cancel = document.createElement("button");
  cancel.type = "button";
  cancel.textContent = "Annuler";

  title.replaceWith(input);
  for (const button of item.querySelectorAll("button")) {
    button.disabled = true;
  }
  input.after(save);
  save.after(cancel);
  input.focus();

  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      save.click();
    } else if (event.key === "Escape") {
      event.preventDefault();
      cancel.click();
    }
  });

  save.addEventListener("click", () => {
    const value = input.value;
    mutate(async () => {
      await api("PATCH", `/api/tasks/${task.id}`, { title: value });
    });
  });
  cancel.addEventListener("click", () => {
    clearError();
    refresh();
  });
}

async function refresh() {
  try {
    const payload = await api("GET", "/api/tasks");
    render(payload.tasks);
  } catch (error) {
    showError(error.message);
  }
}

async function mutate(action) {
  clearError();
  if (!compatible) {
    showError("client incompatible : mutation refusée localement");
    return;
  }
  try {
    await action();
    await refresh();
  } catch (error) {
    showError(error.message);
  }
}

form.addEventListener("invalid", (event) => {
  // The browser blocks submission of an empty required field before `submit` fires;
  // this keeps the failure visible in the DOM instead of only in a native bubble.
  if (event.target === titleInput) {
    showError("le titre est obligatoire");
  }
}, true);

form.addEventListener("submit", (event) => {
  event.preventDefault();
  const title = titleInput.value;
  const due = dueInput.value || null;
  mutate(async () => {
    await api("POST", "/api/tasks", { title, due_date: due });
    titleInput.value = "";
    dueInput.value = "";
  });
});

refreshButton.addEventListener("click", () => {
  clearError();
  refresh();
});

(async function start() {
  try {
    await checkContract();
    await refresh();
  } catch (error) {
    showError(`démarrage impossible : ${error.message}`);
  } finally {
    // Explicit readiness signal for the browser E2E harness. Without it, a test can
    // interact before the first list render finishes and read a half-initialised DOM;
    // that was a real intermittent failure of scenario 5 in CI (full Chrome, slower
    // runner). Readiness means "the initial contract check and list refresh completed",
    // not that the app is healthy: an error is displayed and asserted separately.
    document.documentElement.dataset.ravumReady = "1";
  }
})();
