# Speed Precise v1.0.0

**Calibração Nativa DirectInput8 de Volante e Pedais, Ensaio Force Feedback `Step Log 2` (1000 Hz) e Gerador Automático de LUT em um Único Executável Portátil para Windows.**

![Plataforma](https://img.shields.io/badge/Plataforma-Windows%2010%20%2F%2011%20(x64)-222222?style=flat-square)
![Linguagem](https://img.shields.io/badge/Rust-2021%20Edition-303030?style=flat-square)
![Portátil](https://img.shields.io/badge/Distribuicao-100%25%20Port%C3%A1til%20(.exe)-27ae60?style=flat-square)
![Licença](https://img.shields.io/badge/Licen%C3%A7a-MIT-404040?style=flat-square)

---

## Visão Geral

O **Speed Precise v1.0** foi desenvolvido para substituir e unificar todo o fluxo de calibração de pedais, análise de Force Feedback e geração de curvas LUT em **um único aplicativo nativo em Rust (`SpeedPrecise.exe`)**, 100% portátil, sem instaladores ou dependências de `.NET` ou `VC++ Redistributable`, projetado com foco em precisão de nível de driver e segurança mecânica para volantes USB DirectInput8 (Logitech G25/G27/G29/G920/G923, Thrustmaster, Fanatec e pedaleiras USB dedicadas).

A calibração de pedais, a análise da resposta dos motores de Force Feedback e a geração de curvas LUT são tarefas tradicionalmente distribuídas entre **três utilitários independentes** — *DIView* (para calibrar curso e zona morta de pedais no driver DirectInput), *WheelCheck* (para medir a resposta não-linear e a zona morta mecânica dos motores de Force Feedback) e *LUT Generator* (para converter o `log2.csv` em uma tabela `.lut` para o Assetto Corsa).

---

## Download & Uso Rápido (Portátil)

Não é necessário instalar nada no Windows:

1. Acesse a página de [**Releases no GitHub**](../../releases) e baixe o pacote **`SpeedPrecise-v1.0.0-win64-portable.zip`**.
2. Extraia o conteúdo do arquivo `.zip` em qualquer pasta de sua preferência.
3. Conecte seu volante e/ou pedaleira USB ao computador e execute **`SpeedPrecise.exe`**.

> **Verificação de Integridade (Opcional):** O arquivo `SpeedPrecise-v1.0.0-win64-portable.zip.sha256` disponibilizado junto à Release permite validar o hash SHA-256 do pacote baixado via PowerShell:
> ```powershell
> Get-FileHash .\SpeedPrecise-v1.0.0-win64-portable.zip -Algorithm SHA256
> ```

---

## Principais Recursos

### 1. Painel de Cockpit (`Volante e Pedais`)
- **Telemetria Simultânea *Raw* + *Cooked* em Tempo Real (`60 Hz`):** Abre dois handles DirectInput8 independentes (`DIPROP_CALIBRATIONMODE = 0x0A`) para exibir lado a lado a leitura física pura do sensor óptico/potenciômetro/célula de carga e o sinal final processado que o Windows entrega aos jogos.
- **Medidor Horizontal de Direção Centralizado:** Escala simétrica em graus (`-450° .. 0° .. +450°`, ajustável para `540°`, `900°`, `1080°` ou valor personalizado).
- **Três Colunas Verticais de Pedais em Ordem de Cockpit:**
  1. **Embreagem** (Esquerda)
  2. **Freio** (Centro)
  3. **Acelerador** (Direita)
  - Cada pedal conta com duas barras verticais: uma coluna estreita indicando o **Curso Físico** real do pedal e uma coluna principal larga exibindo a **Saída no Jogo (`0%` a `100%`)**, com detecção automática de sensores invertidos (como a linha Logitech G29/G920, cujo repouso físico é `255` e pressão máxima é `0`).
- **Saturação Inteligente (`Curso Máximo`) e `Zona Morta Inicial` via `0x0B`:**
  - Diferente do `DIPROP_SATURATION` padrão do DirectInput (que comprime os dois extremos em torno do centro e distorce pedais unilaterais), o **Speed Precise** recalcula automaticamente os 3 pontos `[lMin, lCenter, lMax]` e grava diretamente via `SetProperty(DIPROP_CALIBRATION /* 0x0B */)` no driver/Registro do Windows — permitindo atingir `100%` de frenagem no simulador com, por exemplo, `65%` de curso em borrachas rígidas ou *load cells*, valendo globalmente para qualquer jogo DirectInput.
- **Backup Automático de Segurança:** Antes de gravar qualquer ajuste de calibração para um dispositivo `VID:PID`, o estado anterior é salvo automaticamente em JSON em `%APPDATA%\SpeedPrecise\backups\`, com botão **"Restaurar Padrão dos Pedais"** em 1 clique.

### 2. Gerador Automático de LUT em 1 Clique (`Force Feedback 1000 Hz`)
- **Ensaio `Step Log 2` de Alta Precisão (`1000 Hz`):**
  - Executa a máquina de estados completa do `Step Log 2` com temporização de alta resolução do kernel do Windows (`timeBeginPeriod(1)` + `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION`), controle PID auto-centralizante (`~58 segundos` para 50 passos) e deduplicação de pacotes USB HID `IDirectInputEffect::SetParameters`.
- **Pipeline Unificado de Curva LUT Inversa (`101 Pontos`):**
  - Ao concluir o último passo (ou ao importar um `.csv` existente do *WheelCheck*), gera instantaneamente na mesma tela a curva de compensação `.lut` (`0.00` a `1.00`), calculando a zona morta mecânica exata do volante, removendo ruídos não-monotônicos e aplicando suavização simétrica sem distorcer o ponto de pré-carga inicial.
- **Gráfico Interativo Confinado (`[0.0, 1.0] × [0.0, 1.0]`):**
  - Compara a *Diagonal Linear Ideal (1:1)*, a *Resposta Bruta do Volante* e a *Curva LUT Corrigida (Final)* com zoom via roda do mouse centralizado no cursor e botão **Resetar Zoom**.
- **Aplicação Direta no Assetto Corsa:**
  - O botão **"Aplicar no Assetto Corsa"** grava o arquivo `speed_precise.lut` em `Documents\Assetto Corsa\cfg\`, cria backup automático (`ff_post_process.ini.bak`) e ativa `TYPE=LUT`, `ENABLED=1` e `CURVE=speed_precise.lut` preservando demais parâmetros do usuário. Também permite salvar o arquivo `.lut` livremente para **BeamNG.drive** e outros simuladores.

### 3. Segurança Mecânica (`FfbSafetyGuard`)
- **Corte Instantâneo de Emergência (`< 1 ms`):** Pressionar **`ESPAÇO`**, **`ESC`** ou clicar no botão vermelho **"Parar Teste Agora"** interrompe imediatamente o laço de `1000 Hz` via flag atômica (`Arc<AtomicBool>`), zera o torque e executa `IDirectInputEffect::Stop()` + `Unacquire()`.
- **Proteção RAII e Envelope de Fim de Curso:** O destrutor (`Drop`) de `FfbSession` desenergiza o motor incondicionalmente em qualquer encerramento normal ou erro, além de contar com limite preventivo de rotação (`85%` do batente) e detecção anti-*runaway*.

---

## Ferramenta de Linha de Comando (`speed-precise-cli.exe`)

O pacote portátil também inclui o binário `speed-precise-cli.exe` para automação, diagnóstico e uso avançado via terminal:

```powershell
# Listar controladores DirectInput8, eixos, faixas lógicas e calibração atual
.\speed-precise-cli.exe list

# Monitorar telemetria Raw vs. Cooked em tempo real no terminal
.\speed-precise-cli.exe monitor --device 0

# Executar o ensaio completo Step Log 2 + geração de LUT + injeção no Assetto Corsa
.\speed-precise-cli.exe auto-tune --device 0 --steps 50 --max-angle 900 --inject-ac

# Simular todo o pipeline sem hardware físico conectado (modo Dry-Run)
.\speed-precise-cli.exe auto-tune --dry-run --steps 50 --output-csv log2.csv --output-lut speed_precise.lut
```

---

## Arquitetura do Workspace & Compilação a partir do Código-Fonte

O projeto é estruturado como um workspace Cargo modular em Rust (`Edition 2021`):

```text
speed-precise/
├── crates/
│   ├── speed-precise-core/   # FFI DirectInput8 64-bit, calibração 0x0B, motor FFB 1000 Hz, LUT & injeção INI
│   ├── speed-precise-gui/    # Interface desktop nativa (eframe/egui/egui_plot) + worker thread isolada
│   └── speed-precise-cli/    # Interface de linha de comando (clap + ctrlc)
├── build_portable_release.py # Script reprodutível de empacotamento portátil (.exe + .zip + .sha256)
└── run_regression_tests.py   # Suíte mestre determinística de testes de regressão (11 estágios)
```

### Pré-requisitos para Compilar
- **Windows 10 ou Windows 11 (x64)**
- **Rust Stable Toolchain** (`x86_64-pc-windows-msvc`, `rustc >= 1.80`) com *Visual Studio C++ Build Tools*
- **Python 3.10+** (opcional, para executar a suíte de regressão e o empacotador `.zip`)

### Comandos de Build e Testes
```powershell
# Executar a suíte completa de testes unitários e de regressão (0 warnings)
python run_regression_tests.py

# Compilar e gerar o pacote portátil completo em dist/SpeedPrecise-v1.0.0-win64-portable.zip
python build_portable_release.py
```

---

## Licença

Distribuído sob a licença **MIT**. Consulte o arquivo [`LICENSE`](LICENSE) para mais detalhes.
