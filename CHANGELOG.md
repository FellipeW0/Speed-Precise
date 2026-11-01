# Changelog

Todas as mudanças notáveis do projeto **Speed Precise** são documentadas neste arquivo.

O formato segue os princípios do [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/) e este projeto adere ao [Versionamento Semântico](https://semver.org/lang/pt-BR/).

---

## [1.0.0] - 2026-11-01

### Adicionado
- **Aplicativo Desktop Nativo 100% Portátil (`SpeedPrecise.exe`):**
  - Unificação completa das funcionalidades do *DIView*, *WheelCheck (`Step Log 2`)* e *LUT Generator* em um único executável nativo Windows x64 escrito em Rust (`eframe` / `egui` / `egui_plot`), compilado com CRT estático (`+crt-static`), LTO completo e subsistema gráfico puro (`windows_subsystem = "windows"`, sem janela de console em segundo plano).
- **Painel de Cockpit Sim-Racing (`Volante e Pedais`):**
  - Medidor horizontal centralizado de direção (`-450° .. 0° .. +450°`, adaptável a `540°`, `900°`, `1080°` ou ângulo customizado) com leitura simultânea de eixos DirectInput8 em modo Bruto (*Raw* via `DIPROP_CALIBRATIONMODE = 0x0A`) e Calibrado (*Cooked*).
  - Três colunas verticais dedicadas em ordem real de cockpit (**Embreagem**, **Freio** e **Acelerador**) com barras duplas independentes para curso físico do sensor e sinal final enviado ao simulador (`0%..100%`), detecção automática de polaridade invertida (padrão Logitech G27/G29/G920/G923) e remapeamento rápido de eixos.
  - **Saturação Inteligente e Zona Morta Nativa no Driver (`DIPROP_CALIBRATION = 0x0B`):** recálculo automático da tripla `[lMin, lCenter, lMax]` diretamente no Registro/driver DirectInput8 do Windows, contornando a distorção simétrica do `DIPROP_SATURATION` padrão e eliminando o travamento transitório em `50%` (`32767`) do `dinput8.dll` após chamadas `SetProperty`.
  - **Backup Automático de Segurança:** persistência automática do estado anterior de calibração em JSON (`%APPDATA%\SpeedPrecise\backups\`) antes de qualquer gravação para cada par `VID:PID` e eixo, com restauração de fábrica em 1 clique.
- **Estúdio Auto-LUT em 1 Clique (`Gerador de LUT — Force Feedback`):**
  - Motor de varredura de linearidade `Step Log 2` operando em malha fechada real a **`1000 Hz`** (`HighResTimerGuard` com `timeBeginPeriod(1)`, `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION` e controle PID com derivada filtrada para retorno rápido ao centro em `~58s` para 50 passos).
  - Cálculo matemático imediato da curva LUT inversa de 101 pontos (`0.00..1.00`), detecção automática de zona morta mecânica do motor FFB, imposição de monotonicidade estrita e suavização simétrica preservadora de extremos.
  - Gráfico interativo de 3 curvas (*Linear Ideal 1:1*, *Resposta Bruta do Volante* e *Curva LUT Corrigida*) estritamente confinado ao quadrado unitário `[0.0, 1.0] × [0.0, 1.0]` com zoom centralizado no cursor e botão de reset.
  - **Injeção Automática em Simuladores:** exportação direta de `.lut` e configuração automática do `ff_post_process.ini` para **Assetto Corsa** (com backup `.bak`), além de suporte a **BeamNG.drive** e importação/exportação de arquivos `.csv` compatíveis com *WheelCheck*.
- **Segurança Física de Hardware (`FfbSession` RAII & Corte `< 1 ms`):**
  - Corte instantâneo de torque (`Stop()` + `Unacquire()`) garantido via `Drop` (RAII), atalhos globais de teclado (`ESPAÇO` e `ESC`), botão contextual de parada de emergência durante testes ativos e envelope de segurança contra fim de curso mecânico (`85%` / `±382.5°` em `900°`).
- **Interface Visual Refinada:**
  - Paleta escura neutra em tons de preto e cinza-escuro (`R == G == B`, sem dominante azulada) com contraste de alta legibilidade (acima de WCAG AA) e iconografia 100% vetorial desenhada via `egui::Painter` (zero dependência de glifos emoji ou símbolos ausentes em fontes de sistema).
- **Ferramenta de Linha de Comando (`speed-precise-cli.exe`):**
  - Subcomandos `list`, `monitor`, `calibrate`, `step-log2`, `generate-lut` e `auto-tune` (incluindo modo `--dry-run` com simulador físico virtual determinístico).
