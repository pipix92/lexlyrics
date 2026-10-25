# NLLB-200 translator daemon

Local HTTP server LexLyrics uses for the quick Italian translation
(`POST http://127.0.0.1:18436/translate`, `GET /wake`, `GET /health`).

It keeps `facebook/nllb-200-distilled-600M` in system RAM while idle and
moves it to the GPU (CUDA) in about 0.7 s when LexLyrics starts, then back to
RAM after a minute without requests, so the VRAM is free for games or other
models.

```bash
pip install --user torch transformers sentencepiece
install -Dm755 nllb-translation-server.py ~/.local/bin/nllb-translation-server.py
# first run only: let it download the model (~2.4 GB)
TRANSFORMERS_OFFLINE=0 HF_HUB_OFFLINE=0 python3 ~/.local/bin/nllb-translation-server.py
# then run it as a user service
install -Dm644 nllb-translator.service ~/.config/systemd/user/nllb-translator.service
systemctl --user enable --now nllb-translator.service
curl -s http://127.0.0.1:18436/health
```
