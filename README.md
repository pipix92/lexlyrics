# 🎵 LexLyrics

<div align="center">
  <h3>Desktop Lyrics with Real-time Neural Translation & 3-Row Upward Karaoke Scroll</h3>
  <p>Fork evoluto di <a href="https://github.com/waylyrics/waylyrics">Waylyrics</a> ottimizzato per Linux (Wayland / COSMIC / GNOME / KDE) con accelerazione hardware NVIDIA CUDA.</p>

  <p>
    <img src="https://img.shields.io/badge/Language-Rust%202021-orange.svg" alt="Rust" />
    <img src="https://img.shields.io/badge/GUI-GTK%204%20%7C%20Libadwaita-blue.svg" alt="GTK4" />
    <img src="https://img.shields.io/badge/AI%20Engine-NLLB--200%20%7C%20CUDA%20FP16-green.svg" alt="CUDA" />
    <img src="https://img.shields.io/badge/VRAM%20Tiering-Zero%20MB%20Idle-purple.svg" alt="VRAM" />
    <img src="https://img.shields.io/badge/License-GPL--3.0-blue.svg" alt="License" />
  </p>
</div>

---

## 🌟 Nuove Funzionalità di LexLyrics (Rispetto ad Upstream)

| Funzionalità | Upstream Waylyrics | **LexLyrics Fork** |
| :--- | :--- | :--- |
| **Visualizzazione Testi** | Solo 2 righe statiche ([Label; 2]) | **3 Righe con Karaoke Scroll (4 Label: prev, curr orig, curr it, next)** |
| **Traduzione Canzoni** | Solo Cinese (NetEase / QQ Music) | **Italiano & Multilingua Neurale (NLLB-200, MarianMT, Claude, AGY, Hermes)** |
| **Motore AI Locale** | Nessuno | **Server locale NLLB-200 su GPU RTX 3050 (<650ms a brano)** |
| **Gestione Risorse GPU** | Non applicabile | **Dynamic VRAM Tiering (0 MB a riposo, promotion a CUDA in 0.7s all'avvio)** |
| **Filtro Anti-Cinese** | Assente (mostra ideogrammi cinesi) | **Istantaneo (<50ms): sostituisce con traduzione o testo pulito** |
| **Integrazione Dashboard** | Assente | **Telemetria in tempo reale + Hotkey [T] in `ai-tui`** |

---

## 🖥️ Layout Visivo (3-Row Upward Scroll)

```
   ┌────────────────────────────────────────────────────────┐
   │                                                        │
   │  [prev]   Yesterday all my troubles seemed so far away │  <- 40% Opacità, Font compatto
   │                                                        │
   │  [above]  NOW IT LOOKS AS THOUGH THEY'RE HERE TO STAY  │  <- 100% Bianco, Bold, Verso Attivo
   │  [below]  Ora sembra proprio che siano qui per restare │  <- 95% Azzurro, Traduzione Italiana
   │                                                        │
   │  [next]   Oh, I believe in yesterday                   │  <- 40% Opacità, Font compatto
   │                                                        │
   └────────────────────────────────────────────────────────┘
```

Grazie alle transizioni CSS native di GTK4 (`cubic-bezier(0.25, 0.1, 0.25, 1.0)`), ogni verso scorre morbidamente verso l'alto senza scatti o artefatti visivi.

---

## 🏗️ Architettura del Sistema

```mermaid
flowchart TD
    Player["🎵 Player Audio (Spotify, VLC, MPD)"] -->|MPRIS DBus| LexLyrics["🖥️ LexLyrics (GTK4 / Rust)"]
    
    subgraph Core ["Motore di Sincronizzazione"]
        LexLyrics --> NetEase["🌐 Lyric Fetcher (NetEase / QQ)"]
        NetEase --> AntiChinese["🛡️ Anti-Chinese Filter"]
        AntiChinese --> TransEngine["🌐 Pluggable Translation Engine"]
    end

    subgraph Backends ["Backend di Traduzione Configurabili"]
        TransEngine -->|Opzione 1 (Default)| NLLB["🚀 NLLB-200 Distilled 600M (CUDA HTTP 18436)"]
        TransEngine -->|Opzione 2| Marian["⚡ MarianMT (opus-mt-en-it)"]
        TransEngine -->|Opzione 3| AGY["🤖 AGY CLI (agy -p)"]
        TransEngine -->|Opzione 4| Claude["🧠 Claude CLI (claude -p)"]
        TransEngine -->|Opzione 5| Hermes["⚡ Hermes CLI (hermes -p)"]
    end

    subgraph VRAM ["Dynamic VRAM Tiering"]
        NLLB -.->|App Aperta / Demand| CUDA["🎮 RTX 3050 VRAM (~1.1 GB)"]
        NLLB -.->|App Chiusa / Idle > 75s| RAM["💾 System RAM (0 MB VRAM)"]
    end

    TransEngine --> UI["📱 4-Label Upward Scrolling UI"]
```

---

## ⚙️ Configurazione (`config.toml`)

Aggiungi o personalizza la sezione `[translation]` nel file di configurazione (`~/.config/waylyrics/config.toml`):

```toml
[translation]
# Backend preferito: "nllb" | "marian" | "agy" | "claude" | "hermes"
backend = "nllb"

# URL dell'endpoint neurale locale
nllb_url = "http://127.0.0.1:18436/translate"

# Lingua di destinazione (codice ISO NLLB o standard)
target_lang = "ita_Latn"

# Timeout massimo per batch prima del fallback a cascata (millisecondi)
timeout_ms = 2500

# Abilita il layout a 3 versi contemporanei (prev / curr / next)
multi_row_scroll = true

# Durata transizione CSS scorrimento (millisecondi)
animation_speed_ms = 350
```

---

## 🚀 Compilazione & Installazione

### Requisiti di Sistema
- **Rust & Cargo**: >= 1.80
- **GTK 4 & Libadwaita**: `libgtk-4-dev`, `libadwaita-1-dev`
- **DBus & OpenSSL**: `libdbus-1-dev`, `libssl-dev`

### Compilazione del Binario
```bash
# Clona il repository
git clone https://github.com/lex/lexlyrics.git
cd lexlyrics

# Compila in modalità Release ultra-ottimizzata
cargo build --release

# Il binario compilato si trova in:
# target/release/waylyrics (o lexlyrics)
```

### Avvio con il Demone di Traduzione Locale NLLB-200
```bash
# Avvia il server neurale locale (se non già gestito da systemd)
systemctl --user start nllb-translator.service

# Avvia LexLyrics
./target/release/waylyrics
```

---

## 🤝 Riconoscimenti & Licenza
- Progetto basato sul lavoro originale di [Waylyrics](https://github.com/waylyrics/waylyrics).
- Licenza: **GPL-3.0**.
