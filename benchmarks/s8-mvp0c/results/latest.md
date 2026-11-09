# S8 MVP-0c — pilote scripté : résultats bruts (généré automatiquement)

Généré le 2026-11-09T00:52:33.855Z. Mode : **pilot**.

> Les agents du pilote sont des **éditeurs scriptés déterministes**, pas des LLM :
> les tokens IA ne sont pas mesurés (aucune session DSH produite) et le temps de
> supervision humaine n’est pas mesuré. Aucun chiffre n’est inventé.

## Environnement

```json
{
  "generated_at": "2026-11-09T00:51:47.804Z",
  "machine": {
    "source": "node-os",
    "cpu_model": "13th Gen Intel(R) Core(TM) i7-13620H",
    "logical_cores": 16,
    "physical_cores": null,
    "physical_cores_note": "non lisible sans WMI (indisponible pendant la mission) : seule la valeur logique est mesurée",
    "total_ram_bytes": 16868962304,
    "os_platform": "win32",
    "os_release": "10.0.26100",
    "os_version": "Windows 11 Pro",
    "hostname": "MSI",
    "versions": {
      "node": "v26.13.0",
      "cargo": "cargo 1.95.0 (f2d3ce0bd 2026-03-21)",
      "rustc": "rustc 1.95.0 (59807616e 2026-04-14)",
      "git": "git version 2.51.1.windows.1"
    },
    "disk": {
      "free_bytes": 201321607168,
      "total_bytes": 2794898583552
    },
    "node": {
      "version": "v26.13.0",
      "platform": "win32",
      "arch": "x64"
    }
  },
  "versions": {
    "node": "v26.13.0",
    "cargo": "cargo 1.95.0 (f2d3ce0bd 2026-03-21)",
    "rustc": "rustc 1.95.0 (59807616e 2026-04-14)",
    "git": "git version 2.51.1.windows.1"
  },
  "disk": {
    "free_bytes": 201321607168,
    "total_bytes": 2794898583552
  },
  "load_probes": [
    {
      "label": "début de campagne",
      "ok": true,
      "source": "node-os",
      "ms": 262.93,
      "epoch_ms": 1794185508996,
      "free_ram_bytes": 2706984960,
      "total_ram_bytes": 16868962304,
      "load_pct": 21.3,
      "logical_cpus": 16
    },
    {
      "label": "mi-campagne",
      "ok": true,
      "source": "node-os",
      "ms": 261.47,
      "epoch_ms": 1794185528958,
      "free_ram_bytes": 2774048768,
      "total_ram_bytes": 16868962304,
      "load_pct": 34.4,
      "logical_cpus": 16
    },
    {
      "label": "fin de campagne",
      "ok": true,
      "source": "node-os",
      "ms": 262.927,
      "epoch_ms": 1794185553817,
      "free_ram_bytes": 2270220288,
      "total_ram_bytes": 16868962304,
      "load_pct": 35.8,
      "logical_cpus": 16
    }
  ],
  "allow_network": false,
  "cargo_build_jobs": 2
}
```

## Fixture (tests rouges avant éditions)

```json
{
  "ok": true,
  "ms": 1330.8,
  "passed": 5,
  "failed": 7,
  "failed_names": [
    "mean_of_negative_values",
    "mean_of_simple_values",
    "median_of_even_list",
    "median_of_odd_list",
    "parse_iso_date_accepts_valid_date",
    "slugify_basic_title",
    "slugify_collapses_separators_and_trims"
  ],
  "note": "les tests doivent être rouges avant les éditions ; le harnais échoue sinon"
}
```

## Sonde de détection de conflits (contrôle positif)

```json
{
  "ok": true,
  "ms": 1.88,
  "description": "writer-1 écrit stats.rs (moyenne), puis writer-2 écrit stats.rs calculé depuis la base (médiane) : l’édition de writer-1 est écrasée. Le détecteur doit la signaler.",
  "intents": [
    {
      "agent": "writer-1",
      "path": "src/stats.rs",
      "final_sha256": "e2866f7fd315fcdb8ee04dee0d7fa377ef1ec2b21b6d68f1fe4ffaf9c4535a4b"
    },
    {
      "agent": "writer-2",
      "path": "src/stats.rs",
      "final_sha256": "066b4d1103cbc01af404e9f8ec9d5de547da58fe8161ff5f0b651893545bfe7f"
    }
  ],
  "final_sha256": "066b4d1103cbc01af404e9f8ec9d5de547da58fe8161ff5f0b651893545bfe7f",
  "clobbered_writers": [
    "writer-1"
  ],
  "final_equals_last_writer": true
}
```

## Tableau comparatif (médiane [min–max] (IQR), n = 3)

| Métrique | A | B | C |
|---|---|---|---|
| Temps total jusqu’à tests verts (ms) | 2434.37 [2162.25–3118.15] (IQR 477.95) | 2825.16 [2484.12–3168.79] (IQR 342.34) | 2656.58 [2505.14–2838.22] (IQR 166.54) |
| Phase d’édition des agents (ms) | 203.94 [199.19–256] (IQR 28.4) | 483.13 [419.81–496.08] (IQR 38.14) | 461.89 [344.8–463.39] (IQR 59.29) |
| Phase d’intégration (ms) | 19.24 [14.33–22.28] (IQR 3.98) | 0 [0–0] (IQR 0) | 0 [0–0] (IQR 0) |
| cargo test final (ms) | 2188.19 [1928.63–2814.3] (IQR 442.84) | 2315.44 [2055.45–2674.38] (IQR 309.47) | 2181.97 [2152.87–2368.84] (IQR 107.99) |
| Attente cumulée des agents (ms) | 149.14 [121.68–186.23] (IQR 32.27) | 313.57 [301.84–318.15] (IQR 8.16) | 493.74 [364.92–495.75] (IQR 65.42) |
| Conflits détectés | 0 [0–0] (IQR 0) | 1 [1–1] (IQR 0) | 0 [0–0] (IQR 0) |
| Temps de résolution des conflits (ms) | 0.41 [0.33–1.03] (IQR 0.35) | 313.57 [301.84–318.15] (IQR 8.16) | 0 [0–0] (IQR 0) |
| Fusions / intégrations | 4 [4–4] (IQR 0) | 0 [0–0] (IQR 0) | 0 [0–0] (IQR 0) |
| Écritures / tentatives | 8 [8–8] (IQR 0) | 5 [5–5] (IQR 0) | 4 [4–4] (IQR 0) |
| Itérations d’agents (tentatives) | 4 [4–4] (IQR 0) | 5 [5–5] (IQR 0) | 4 [4–4] (IQR 0) |
| Compilations cargo | 1 [1–1] (IQR 0) | 1 [1–1] (IQR 0) | 1 [1–1] (IQR 0) |
| Tests verts (sur 12) | 12 [12–12] (IQR 0) | 12 [12–12] (IQR 0) | 12 [12–12] (IQR 0) |
| Changements perdus | 0 [0–0] (IQR 0) | 0 [0–0] (IQR 0) | 0 [0–0] (IQR 0) |
| CPU des processus suivis (s) | 0.16 [0.06–0.31] (IQR 0.13) | 0.25 [0.11–0.38] (IQR 0.13) | 0.05 [0.02–0.05] (IQR 0.02) |
| RAM pic des processus suivis (Mo WorkingSet) | 44.2 [10.6–90.6] (IQR 40) | 133.2 [62.3–133.8] (IQR 35.75) | 10.9 [10.6–11.4] (IQR 0.4) |
| RAM des agents (RSS auto-déclaré, Mo) | 44.2 [43.7–44.2] (IQR 0.25) | 44.8 [44.6–44.9] (IQR 0.15) | 44.3 [44.1–44.3] (IQR 0.1) |
| Charge CPU machine pendant la répétition (%) | 80.8 [61.9–96] (IQR 17.05) | 82.7 [62.8–89.6] (IQR 13.4) | 61 [52.3–72.2] (IQR 9.95) |
| RAM machine libre minimale (Mo) | 2365 [2061–2478] (IQR 208.5) | 2412 [1947–2466] (IQR 259.5) | 2356 [1963–2448] (IQR 242.5) |
| Chute RAM machine pendant la répétition (Mo) | 197 [193–601] (IQR 204) | 181 [150–264] (IQR 57) | 169 [75–216] (IQR 70.5) |
| Cache target/ de la réplique (Mo) | 11.3 [11.3–11.3] (IQR 0) | 11.3 [11.3–11.3] (IQR 0) | 11.3 [11.3–11.3] (IQR 0) |
| Disque de la réplique (Mo) | 11.31 [11.31–11.31] (IQR 0) | 11.3 [11.3–11.3] (IQR 0) | 11.3 [11.3–11.3] (IQR 0) |
| Réussite fonctionnelle (0/1) | 1 [1–1] (IQR 0) | 1 [1–1] (IQR 0) | 1 [1–1] (IQR 0) |

## Échantillons bruts

| rep | ordre | total_ms | edit_ms | integr_ms | test_ms | attente_ms | conflits | résol_ms | itérations | tests | perdus | RAM_Mo | CPU_s | target_Mo | succès |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A1 | 1 | 2162.252 | 199.189 | 14.327 | 1928.629 | 121.683 | 0 | 1.031 | 4 | 12/12 | 0 | 10.6 | 0.062 | 11.3 | 1 |
| B1 | 2 | 2484.12 | 419.805 | 0 | 2055.446 | 301.841 | 1 | 301.841 | 5 | 12/12 | 0 | 133.8 | 0.25 | 11.3 | 1 |
| C1 | 3 | 2838.216 | 461.888 | 0 | 2368.841 | 493.735 | 0 | 0 | 4 | 12/12 | 0 | 10.6 | 0.016 | 11.3 | 1 |
| C2 | 4 | 2656.583 | 463.387 | 0 | 2181.97 | 495.75 | 0 | 0 | 4 | 12/12 | 0 | 11.4 | 0.047 | 11.3 | 1 |
| B2 | 5 | 2825.159 | 496.078 | 0 | 2315.435 | 313.569 | 1 | 313.569 | 5 | 12/12 | 0 | 133.2 | 0.375 | 11.3 | 1 |
| A2 | 6 | 2434.371 | 203.94 | 19.236 | 2188.191 | 149.138 | 0 | 0.407 | 4 | 12/12 | 0 | 44.2 | 0.156 | 11.3 | 1 |
| A3 | 7 | 3118.149 | 255.995 | 22.283 | 2814.299 | 186.225 | 0 | 0.329 | 4 | 12/12 | 0 | 90.6 | 0.312 | 11.3 | 1 |
| B3 | 8 | 3168.793 | 483.127 | 0 | 2674.378 | 318.154 | 1 | 318.154 | 5 | 12/12 | 0 | 62.3 | 0.109 | 11.3 | 1 |
| C3 | 9 | 2505.143 | 344.8 | 0 | 2152.872 | 364.92 | 0 | 0 | 4 | 12/12 | 0 | 10.9 | 0.047 | 11.3 | 1 |

## Tokens et supervision

```json
{
  "tokens": {
    "measured": false,
    "reason": "pilote exécuté avec des éditeurs SCRIPTÉS DÉTERMINISTES : aucun LLM appelé, aucune session DSH produite. Les tokens réels ne sont pas mesurés ici et ne seront mesurés que par la campagne live via tools/agent-usage/usage.mjs.",
    "per_agent": null,
    "tool": "tools/agent-usage/usage.mjs (campagne live uniquement)"
  },
  "human_supervision": {
    "measured": false,
    "reason": "pilote scripté ; le temps de supervision humaine de la campagne réelle sera mesuré par le Lead"
  }
}
```

## Validation du protocole

```json
{
  "ok": true,
  "problems": []
}
```

