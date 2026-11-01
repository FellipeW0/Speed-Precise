# Speed Precise — Fase 1.1: Descobertas Reais da Auditoria Estática dos Binários Descompilados

> **Status do Documento:** Verificado contra os artefatos reais em `decompiled _apps/` (`DIView.exe.c`, `WheelCheck.exe.c`, `LUTGenerator.exe.c`) e respectivas seções de código/metadados PE/CLR dos executáveis originais.
> **Objetivo:** Registrar os endereços de memória exatos, estruturas binárias, constantes `dinput8.dll`, fórmulas matemáticas e discrepâncias em relação à especificação teórica (`Documentation/Documentação_Fase_1.md`), servindo como **Fonte da Verdade Definitiva** para a implementação do `speed-precise-core` em Rust na Fase 2.

---

## 1. Inventário de Artefatos Auditados e Armadilhas de Ambiente

| Aplicativo | Arquivo Descompilado (`decompiled _apps/`) | Tamanho | Arquitetura / Natureza Real |
| :--- | :--- | :--- | :--- |
| **LUT Generator** | `decompiled _apps/LUTGenerator.exe.c` | `165.124 B` (`5.525` linhas) | Executável **.NET Framework 4.0 (WPF / CIL)** (`LUTFinder` / `LUTGenerator`) que delega o cálculo matemático para a DLL externa `LUTLibrary.dll`. |
| **WheelCheck** | `decompiled _apps/WheelCheck.exe.c` | `854.328 B` (`29.757` linhas) | Executável **Win32 C/C++ nativo (x86)** (`ImageBase = 0x00400000`), linkado estaticamente ao CRT e `dinput8.dll` / `winmm.dll`. |
| **DIView** | `decompiled _apps/DIView.exe.c` | `303.220 B` (`12.385` linhas) | Executável **Win32 MFC C++ nativo (x86)** (`DIView v4.65.116`, `ImageBase = 0x00400000`), linkado a `MFC42.DLL`, `ole32.dll` e `HID.DLL`. |

> **Nota de Governança sobre Nomes de Diretórios:**
> No workspace atual (`d:\Speed_Precise_app`), o diretório contendo os arquivos exportados pelo Ghidra chama-se literalmente `decompiled _apps/` (**com um espaço antes do underscore**), e a documentação inicial encontra-se em `Documentation/`.

---

## 2. Auditoria Real: `LUTGenerator` (`LUTGenerator.exe.c` / CIL Bytecode)

### 2.1. Por que `LUTGenerator.exe.c` contém apenas `void entry(void)`
Ao exportar `LUTGenerator.exe` pelo decompilador C nativo do Ghidra (`decompiled _apps/LUTGenerator.exe.c`), o Ghidra processou o cabeçalho PE/CLI como dados estruturados (`MethodDefHdr_Fat`, `MemberRef Row[101]`, `TypeRef Row[85]`, `#Blob` nas linhas `1–5512`) e descompilou apenas o stub nativo x86 na linha `5516`:

```c
// decompiled _apps/LUTGenerator.exe.c (Linhas 5516-5523)
void entry(void)
{
    // WARNING: Could not recover jumptable at 0x004056c6. Too many branches
    // WARNING: Treating indirect jump as call
    _CorExeMain();
    return;
}
```

### 2.2. Desmontagem CIL Completa e Mapeamento de Tokens (`BSJB` @ `0x00000CE0`)
A inspeção estática das tabelas de metadados CLI (`#~` @ `0x00000D4C`, `#Strings` @ `0x00001720`) e do bytecode CIL dos métodos definidos no assembly revelou que `LUTGenerator.exe` é o frontend WPF (`LUTFinder.MainWindow` e `LUTGenerator.SubWindow`) que referencia o assembly externo `LUTLibrary, Version=1.0.0.0`:

#### Tipos Externos Referenciados (`TypeRef` em `LUTLibrary.dll`):
* `TypeRef[0x18]`: `LUTLibrary.LUT`
* `TypeRef[0x37]`: `LUTLibrary.LUTReader`
* `TypeRef[0x38]`: `LUTLibrary.LUTCalculator`

#### Métodos Externos Invocados (`MemberRef` em `LUTLibrary.dll`):
| Token Metadados | Assinatura Reconstruída | Papel no Algoritmo |
| :--- | :--- | :--- |
| `0x0A000024` | `LUT LUTReader::Read(string path)` | Faz o parse do `log2.csv`, extraindo as colunas `force` e `deltaX`/`deltaXDeg`. |
| `0x0A000025` | `int LUT::getMaxDeltaX()` | Calcula/retorna o pico máximo de deslocamento (`R_max`). |
| `0x0A000026` | `LUTCalculator::.ctor(LUT lut)` | Executa a filtragem monotônica, suavização e interpolação inversa preenchendo as listas internas do objeto `LUT`. |
| `0x0A00002B` | `ArrayList LUT::getForce()` | Lista de `int` contendo os comandos de força brutos de cada passo ($0 \dots 10000$). |
| `0x0A00002D` | `int LUT::getMaxForce()` | Força máxima da tabela (normalmente `10000`). |
| `0x0A00002E` | `ArrayList LUT::getLut2()` | Lista de `double` com os valores finais da curva inversa LUT (na escala $0.0 \dots 10000.0$). |
| `0x0A000053` | `ArrayList LUT::getDeltaXRaw()` | Lista de `int` com os valores brutos de `deltaX` lidos do CSV antes da correção de ruído. |
| `0x0A00005A` | `ArrayList LUT::getDeltaX()` | Lista com os valores de `deltaX` após o filtro de monotonicidade/spike. |
| `0x0A000059` | `ArrayList LUT::getRealForce()` | Lista de `double` com a curva suavizada (`wcsmoothed`, na escala $0.0 \dots 10000.0$). |

### 2.3. Código C# Exato Reconstruído do Bytecode CIL

#### `LUTFinder.MainWindow::button1_Click` (Token `0x06000008`, `IL_0000`–`IL_01B5`):
```csharp
private void button1_Click(object sender, RoutedEventArgs e)
{
    try
    {
        OpenFileDialog openFileDialog = new OpenFileDialog();
        openFileDialog.InitialDirectory = Environment.GetFolderPath(Environment.SpecialFolder.Personal); // CSIDL_PERSONAL (5)
        openFileDialog.Filter = "csv files (*.csv)|*.csv";
        openFileDialog.FilterIndex = 2;
        openFileDialog.RestoreDirectory = true;
        if (openFileDialog.ShowDialog() == System.Windows.Forms.DialogResult.OK)
        {
            this.lut = LUTReader.Read(openFileDialog.FileName);
            this.lut.getMaxDeltaX();
            new LUTCalculator(this.lut);

            SaveFileDialog saveFileDialog = new SaveFileDialog();
            saveFileDialog.InitialDirectory = Environment.GetFolderPath(Environment.SpecialFolder.Personal) + "\\Assetto Corsa\\cfg";
            saveFileDialog.Filter = "lut files (*.lut)|*.lut";
            saveFileDialog.FilterIndex = 2;
            saveFileDialog.RestoreDirectory = true;
            if (saveFileDialog.ShowDialog() == System.Windows.Forms.DialogResult.OK && saveFileDialog.FileName != "")
            {
                using (StreamWriter streamWriter = new StreamWriter(saveFileDialog.FileName))
                {
                    for (int i = 0; i < this.lut.getForce().Count; i++)
                    {
                        double xNorm = (double)((int)this.lut.getForce()[i]) / (double)this.lut.getMaxForce();
                        double yLut  = Math.Round((double)this.lut.getLut2()[i] / (double)this.lut.getMaxForce(), 3);
                        streamWriter.WriteLine(
                            xNorm.ToString(CultureInfo.CreateSpecificCulture("en-GB"))
                            + "|"
                            + yLut.ToString(CultureInfo.CreateSpecificCulture("en-GB"))
                        );
                    }
                }
                new SubWindow(this.lut, this).Show();
            }
        }
    }
    catch (Exception ex)
    {
        if (System.Windows.Forms.MessageBox.Show(
                "Error: " + ex.Message,
                "LUT Calculator for AC",
                MessageBoxButtons.OK,
                MessageBoxIcon.Hand,
                MessageBoxDefaultButton.Button1) == System.Windows.Forms.DialogResult.OK)
        {
            Application.Current.Shutdown();
        }
    }
}
```

#### `LUTGenerator.SubWindow::Window_Loaded` (Token `0x0600000E`, `IL_018A`–`IL_0512`):
Plota as 3 curvas (`RosyBrown` = Raw, `Red` = Smoothed, `Green` = LUT Output) e contabiliza os spikes corrigidos (`correctedValues`):
```csharp
Brush[] brushes = new Brush[3] { Brushes.RosyBrown, Brushes.Red, Brushes.Green };

// Curva 0 (RosyBrown): Resposta Bruta (wcraw)
for (int i = 0; i < this.lut.getForce().Count; i++)
{
    int x = (int)this.lut.getForce()[i] / 26;
    double y = ((double)((int)this.lut.getDeltaXRaw()[i] * 10000) / (double)this.lut.getMaxDeltaX()) / 26.0;
    pointsRaw.Add(new Point((double)(x + 10), -y + 390.0));
}

// Curva 1 (Red): Resposta Suavizada (wcsmoothed)
for (int i = 0; i < this.lut.getForce().Count; i++)
{
    int x = (int)this.lut.getForce()[i] / 26;
    double y = (double)this.lut.getRealForce()[i] / 26.0;
    pointsSmoothed.Add(new Point((double)(x + 10), -y + 390.0));
}

// Curva 2 (Green): Saída LUT (lutoutput) + Contagem de Spikes Corrigidos
int correctedCount = 0;
for (int i = 0; i < this.lut.getForce().Count; i++)
{
    if (this.lut.getDeltaXRaw()[i] != this.lut.getDeltaX()[i]) // Comparação de referências/valores boxed
    {
        correctedCount++;
    }
    int x = (int)this.lut.getForce()[i] / 26;
    double y = (double)this.lut.getLut2()[i] / 26.0;
    pointsLut.Add(new Point((double)(x + 10), -y + 390.0));
}
this.correctedValues.Text = correctedCount.ToString();
```

### 2.4. Discrepâncias Descobertas no `LUTGenerator` vs. Especificação Teórica
1. **Quantidade de Linhas do `.lut`:** O `LUTGenerator.exe` **não** gera obrigatoriamente 100 linhas fixas (`0.00` a `1.00`); ele itera sobre `this.lut.getForce().Count`, gerando exatamente $N + 1$ linhas correspondentes aos passos presentes no `log2.csv` de entrada (ex: $51$ linhas se o WheelCheck rodou com `Max Count = 50`, ou $101$ linhas se rodou com `Max Count = 100`).
2. **Arredondamento e Formatação Numérica:** O valor de saída é arredondado para **3 casas decimais** via `Math.Round(lut2[i] / maxForce, 3)` e formatado com `double.ToString(CultureInfo.CreateSpecificCulture("en-GB"))` (formato geral sem zeros à direita forçados, produzindo `0|0` na primeira linha e `1|1` na última). No Rust (`speed-precise-core`), podemos suportar tanto o modo de passos nativos quanto a grade de $100$ pontos (`0.00` a `1.00`), garantindo compatibilidade total com o parser INI/LUT do *Assetto Corsa* e *BeamNG.drive*.

---

## 3. Auditoria Real: `WheelCheck` (`WheelCheck.exe.c`)

### 3.1. Mapa de Funções e Endereços Globais Verificados

| Símbolo Ghidra | Endereço Virtual (VA) | Linhas em `WheelCheck.exe.c` | Responsabilidade |
| :--- | :--- | :--- | :--- |
| `FUN_00401210` | `0x00401210` | `1738–1774` | Loop principal Win32 (`SetPriorityClass(0x80)`, drena até `0x32 = 50` msgs UI, chama `FUN_004078c0()` e executa `Sleep(DAT_00458000)` onde `DAT_00458000 = 1 ms`). |
| `FUN_004024d0` | `0x004024d0` | `2325–2526` | Inicialização da janela, `SetThreadAffinityMask(GetCurrentThread(), 1)`, `timeBeginPeriod(1)` e preenchimento do ComboBox de ensaios (`0x00435380`–`0x004354B0`). |
| `FUN_00403180` | `0x00403180` | `2530–2555` | Chama `DirectInput8Create(GetModuleHandleA(NULL), 0x0800, &IID_IDirectInput8A, &DAT_0045bb4c, NULL)`. |
| `FUN_004034a0` | `0x004034a0` | `2660–2910` | `SelectFFBDevice()`: abre o dispositivo, configura formato, nível cooperativo, propriedades DirectInput e cria os 5 efeitos FFB em `DAT_0045bb60[0..4]`. |
| `FUN_00404040` | `0x00404040` | `3093–3163` | Controlador **PID** completo usado para posicionar/centralizar o volante. |
| `FUN_00404230` | `0x00404230` | `3170–3208` | Wrapper de **Auto-Centering** que invoca o PID `FUN_00404040` e verifica condição de parada. |
| `FUN_004078c0` | `0x004078c0` | `4433–6120` | Tick de telemetria e máquinas de estado de todos os testes FFB. O **`Step Log 2`** é o ramo `DAT_0045bb8c == 0xc` nas **linhas `4962–5090`**. |

#### Variáveis Globais e Valores Padrão (Extraídos da seção `.data` @ `0x00458000`):
* `DAT_00458000` (`0x00458000`): `1` (`Sleep` do loop principal em ms)
* `DAT_00458006` (`0x00458006`): `1` (Flag de direção normal = `1`, invertida = `0`)
* `DAT_00458008` (`0x00458008`): `1` (Quantidade de repetições da bateria de teste)
* `DAT_0045800c` (`0x0045800C`): `10000` (`Max Force`)
* `DAT_00458024` (`0x00458024`): `5.0f` (Ganho Proporcional `Kp` do PID de centralização)
* `_DAT_0045bba8` (`0x0045BBA8`): `0.0f` (Ganho Integral `Ki` do PID de centralização)
* `DAT_00458028` (`0x00458028`): `0.25f` (Ganho Derivativo `Kd` do PID de centralização)
* `DAT_0045802c` (`0x0045802C`): `0.90f` (Constante de filtro derivativo `Td`)
* `DAT_00458030` (`0x00458030`): `10000` (Limite máximo do termo derivativo `maxD`)
* `DAT_00458088` (`0x00458088`): `50` (`Max Count` — número de passos $N$ do `Step Log 2`)
* `DAT_0045808c` (`0x0045808C`): `300` (`Step Time` — duração de cada pulso em ms)
* `DAT_00458090` (`0x00458090`): `900` (`Max Angle` — ângulo total de operação do volante em graus)

### 3.2. Configuração Real do DirectInput em `FUN_004034a0` (`SelectFFBDevice`)
1. **Formato de Dados (`SetDataFormat`, vtable `+0x2C`, linha 2736):**
   Usa `&c_dfDIJoystick` (`0x0043765c`), lendo pacotes de **`0x50` ($80$) bytes** (`DIJOYSTATE`) em `GetDeviceState` (vtable `+0x24`, linha 4665).
2. **Nível de Cooperação (`SetCooperativeLevel`, vtable `+0x34`, linhas 2742–2746):**
   ```c
   local_2a8 = 9; // DISCL_EXCLUSIVE (0x01) | DISCL_BACKGROUND (0x08)
   if (DAT_0045bb82 != '\0') {
       local_2a8 = 5; // DISCL_EXCLUSIVE (0x01) | DISCL_FOREGROUND (0x04)
   }
   ```
   > **Descoberta Crítica:** Ao contrário da crença comum na documentação do DirectInput, o `WheelCheck.exe` usa por padrão `DISCL_EXCLUSIVE | DISCL_BACKGROUND` (`9`), permitindo que o teste continue rodando mesmo se a janela perder o foco, e só usa `DISCL_EXCLUSIVE | DISCL_FOREGROUND` (`5`) se o checkbox *"Foreground"* (`0x415`) estiver marcado!
3. **Configuração da Faixa de Leitura (`DIPROP_LOGICALRANGE` $\to$ `DIPROP_RANGE`, linhas 2777–2802):**
   O `WheelCheck.exe` consulta `GetProperty(DIPROP_LOGICALRANGE /* 0x13 = 19 */)` no eixo X (`dwObj = 0`, `dwHow = DIPH_BYOFFSET = 1`) e **sobrescreve `DIPROP_RANGE` (`4`) com os limites lógicos brutos `[lMin, lMax]` do hardware**!
   Assim, `DIJOYSTATE.lX` (`local_64` / `local_68`) retorna na faixa lógica nativa do encoder (ex: $0 \dots 65535$), e o próprio `WheelCheck.exe` calcula a versão normalizada `local_144` $\in [-10000, +10000]$ em ponto flutuante de 64 bits (`0x00407B18`–`0x00407B4E`):
   $$\text{posX}_{\text{norm}} = \text{local\_144} = \text{trunc}\left( (\text{rawX} - l_{\min}) \times \frac{20000.0}{l_{\max} - l_{\min}} - 10000.0 \right)$$
   Onde $l_{\max} - l_{\min} = \text{local\_168}$ (fallback padrão $= 20000$ caso `GetProperty` falhe).

### 3.3. Máquina de Estados Exata do `Step Log 2` (`FUN_004078c0`, `DAT_0045bb8c == 0xc`, Linhas 4962–5090)

A variável de estado é `DAT_0045be34` (`0` a `5`). Sempre que `DAT_0045be34` muda (`DAT_004580c8 != DAT_0045be34`), o timestamp `local_130 = timeGetTime()` é salvo em `_DAT_0045be30`, e o tempo decorrido no estado atual em milissegundos é:
$$\text{elapsed\_ms} = \text{local\_2e8} = \text{local\_130} - \text{\_DAT\_0045be30}$$

#### Estado `0`: Criação do Arquivo CSV e Elevação de Prioridade (Linhas 4977–5002)
* Obtém a pasta *Meus Documentos* (`SHGetFolderPathA(0, CSIDL_PERSONAL /* 5 */, 0, 0, local_424)`).
* Formata o nome do arquivo como `\log2 %Y-%m-%d %H-%M-%S.csv`.
* Abre o arquivo e grava imediatamente o cabeçalho e a **linha zero literal**:
  ```c
  _fprintf(DAT_0045be2c, "force, startX, endX, deltaX, deltaXDeg\n");
  _fprintf(DAT_0045be2c, "0, 0, 0, 0, 0\n");
  ```
* Eleva o processo para prioridade de tempo real: `SetPriorityClass(GetCurrentProcess(), REALTIME_PRIORITY_CLASS /* 0x100 */)`.
* Zera o contador de passos `DAT_0045be28 = 0` e avança para `DAT_0045be34 = 1`.

#### Estado `1`: Rampa Inicial de Destravamento / Wake-up (Linhas 5003–5021)
* **Não documentado na teoria anterior!** Antes de iniciar o primeiro passo, o WheelCheck aplica uma rampa de força negativa crescente (`local_148 = -DAT_0045be1c`), incrementando `DAT_0045be1c += 50` (`0x32`) a cada ciclo (até o teto de `2000`), monitorando `local_68` (`rawX`).
* Assim que o encoder registra pelo menos 2 variações de posição (`DAT_0045be18 > 1`) **ou** o tempo no estado atinge `2000 ms` (`local_2e8 > 1999`), avança para `DAT_0045be34 = 2`.

#### Estado `2`: Auto-Centralização via Controlador PID (`FUN_00404230`, Linhas 5022–5035 e 3170–3208)
* Chama `FUN_00404230(dt_sec, targetX /* 0 */, posX /* local_144 */, reset_flag, now_ms, &local_148)`.
* Internamente, `FUN_00404230` executa o controlador PID `FUN_00404040`:
  * Erro: $e = \text{posX} - \text{targetX}$ (em unidades $[-10000, +10000]$).
  * Termo Proporcional (`0x004040AF`): $P = e \times K_p$ (com $K_p = 5.0$).
  * Termo Integral (`linhas 3120–3131`): $I = \text{clamp}(I + e \cdot K_i \cdot \Delta t, -1000.0, +1000.0)$ (com $K_i = 0.0$ por padrão).
  * Termo Derivativo filtrado (`linhas 3132–3152`):
    $$\alpha = \text{clamp}\left(\frac{T_d \times 0.001}{\Delta t}, 0.0, 0.99\right), \quad D = (1 - \alpha)\frac{(e - e_{\text{prev}}) K_d}{\Delta t} + \alpha D_{\text{prev}}$$
    limitado a $[-10000, +10000]$.
  * Saída de torque: $\text{force} = \text{clamp}(\text{trunc}(P + I + D), -10000, +10000)$.
* **Critério de Parada da Centralização (Linhas 3191–3206):**
  * Exige um tempo mínimo obrigatório de **`501 ms`** (`uVar3 > 500`).
  * Após `500 ms`, encerra assim que a velocidade angular amortecida cai abaixo do limiar (`fabs(D) < 0.05f`, linha `3195`) **ou** por timeout de **`10.001 ms`** (`uVar3 > 10000`, linha `3203`).
  * Avança para `DAT_0045be34 = 3`.

#### Estado `3`: Acomodação Mecânica com Motor Livre (`local_148 = 0`, Linhas 5036–5042)
* Corta a força (`local_148 = 0`, `DAT_0045bb74 = 1`).
* Aguarda enquanto `local_2e8 <= 0x12a` (`298 ms`), avançando para o Estado `4` quando `local_2e8 > 298` (**$\ge 299\text{ ms} \approx 300\text{ ms}$**, e **não** $400\text{ ms}$).

#### Estado `4`: Captura Inicial, Aplicação do Pulso de Torque e Gravação CSV (Linhas 5043–5072)
1. **No instante inicial (`local_2e8 == 0`):**
   * Salva `DAT_0045be24 = local_144` (`startX` normalizado $\in [-10000, +10000]$).
   * Salva `_DAT_0045be20 = local_68` (`rawStartX` lógico $\in [l_{\min}, l_{\max}]$).
2. **Fórmula da Força do Passo $s \in \{0, \dots, N-1\}$ (Linhas 5048–5049):**
   ```c
   local_148 = (DAT_0045be38 * (int)local_180 * (DAT_0045be28 + 1)) / (int)DAT_00458088;
   ```
   Ou seja, divisão inteira truncada:
   $$F_s = \left\lfloor \frac{\text{dir} \times \text{MaxForce} \times (s + 1)}{N} \right\rfloor$$
3. **Duração do Pulso e Cálculo de `deltaXDeg` (Linhas 5051–5069):**
   * Mantém a força `local_148` ativa até `local_2e8 >= DAT_0045808c` (**`300 ms`** por padrão). *(Nota: Não há trava de fim de curso de 80% no binário original).*
   * Ao atingir `300 ms`:
     ```c
     local_438 = local_144;                                                    // endX [-10000..10000]
     local_430 = local_144 - DAT_0045be24;                                     // endX - startX [-20000..20000]
     local_434 = (float)(-local_430 * DAT_00458090) / (float)local_bc;         // Calculado mas NÃO utilizado!
     local_43c = local_68;                                                     // rawEndX [lMin..lMax]
     local_42c = local_68 - _DAT_0045be20;                                     // rawEndX - rawStartX
     local_428 = ((float)DAT_00458090 / (float)local_168) * (float)-local_42c; // deltaXDeg a partir do RAW!
     _fprintf(DAT_0045be2c, "%d, %d, %d, %d, %f\n",
              (int)local_148,
              -DAT_0045be24,
              -local_144,
              -local_430,
              (double)((float)DAT_0045be38 * local_428));
     ```
   * Incrementa `DAT_0045be28++`. Se `DAT_0045be28 < DAT_00458088` ($50$), volta diretamente para o **Estado `2` (`PID Centering`)**; caso contrário, avança para o **Estado `5`**.

#### Estado `5`: Encerramento e Restauração de Prioridade (Linhas 5073–5089)
* Fecha o arquivo CSV (`_fclose(DAT_0045be2c)`).
* Se todas as repetições (`DAT_00458008 = 1`) terminaram, restaura a prioridade do processo para `HIGH_PRIORITY_CLASS` (`0x80`), zera `DAT_0045bb8c = 0` e envia `CB_SETCURSEL` (`0x14E`) `0` para o ComboBox.

### 3.4. Discrepâncias Críticas no `WheelCheck` vs. Especificação Teórica
1. **Unidades de `startX`, `endX` e `deltaX` no `log2.csv`:**
   Na especificação teórica (Seção 3.4), `startX`, `endX` e `deltaX` estavam descritos como `float` normalizados em $[-1.0, +1.0]$ (`0, -0.0012, -0.0012, 0.0000, 0.0000`).
   **No binário real (linha 5059):** A string de formatação é `"%d, %d, %d, %d, %f\n"`! Os campos `force`, `startX`, `endX` e `deltaX` são **inteiros (`i32`) na escala DirectInput $[-10000, +10000]$** (com sinal invertido `-startX`, `-endX`, `-deltaX`), e apenas `deltaXDeg` é `float` (`%f`)! É por isso que `LUTLibrary.dll` faz *unbox* de `deltaXRaw` como `System.Int32`!
2. **Fórmula de `deltaXDeg` usa Unidades Lógicas Brutas (`local_68`), não `local_144`:**
   Embora a linha `5054` calcule `local_434 = (-local_430 * MaxAngleDeg) / 20000.0f`, essa variável é descartada! O valor gravado no CSV é `local_428`, calculado diretamente da diferença bruta do sensor `local_42c = rawEndX - rawStartX` dividida pelo alcance lógico `local_168 = lMax - lMin`:
   $$\Delta X_{\text{deg}} = \text{dir} \times \left( \frac{\text{MaxAngleDeg}}{l_{\max} - l_{\min}} \times -(\text{rawEndX} - \text{rawStartX}) \right)$$
   Isso preserva a resolução total de 16 bits ($65535$ contagens) do encoder sem sofrer quantização para $20000$ passos.
3. **Sequência de Estados e Tempos:**
   * Existe um estado `1` de *wake-up ramp* antes do primeiro passo.
   * O retorno ao centro (Estado `2`) usa um controlador **PID completo** por no mínimo `501 ms` até `|D_term| < 0.05` (timeout `10000 ms`), seguido de `300 ms` de acomodação sem força (Estado `3`, `0x12a = 298 ms`) **antes** do pulso (Estado `4`, `300 ms`), retornando imediatamente ao Estado `2` após o pulso (sem cooldown pós-pulso separado).
4. **Polaridade Real do Motor DirectInput8 (`GUID_ConstantForce`), Direção Cartesiana (`rglDirection[0] = 1`) e Sinal do Erro PID (`posX - targetX`):**
   * Em `FUN_004034a0` (linha `2826`, `local_33c = 1`) e `FUN_004078c0` (linha `5903`, `local_174 = 1`), o vetor de direção `rglDirection` para `DIEFF_CARTESIAN` (`0x10`) tem o componente X igual a **`+1`** (`1`, e nunca `0`).
   * Em volantes DirectInput8 (como Logitech G29/G920 com `LogicalRange = [0, 65535]` e centro físico $0^\circ = 32768$):
     * Uma magnitude **positiva** (`lMagnitude > 0`) em `DICONSTANTFORCE` puxa o volante para a **ESQUERDA** (reduzindo `rawX` em direção a `lMin = 0`, ou seja, `-10000`).
     * Uma magnitude **negativa** (`lMagnitude < 0`) puxa o volante para a **DIREITA** (aumentando `rawX` em direção a `lMax = 65535`, ou seja, `+10000`).
   * Por essa razão:
     * Na linha `4965`, `DAT_0045be38 = (-(uint)(DAT_00458006 != '\0') & 2) - 1` resulta em **`+1`** quando `DAT_00458006 == 1` (`0xFFFFFFFF & 2 == 2`, `2 - 1 == +1`). Assim, no Estado `4` (linha `5048`), `local_148` é **positivo** (`+200 .. +10000`), girando o volante para a esquerda (`rawEndX < rawStartX`), o que torna `-(rawEndX - rawStartX) > 0` e `-deltaX > 0` no CSV (linhas `5057–5060`)!
     * Em `FUN_00404040` (linha `3108`), o erro do PID é calculado como `iVar2 = param_3 - param_2` (**`err = posX - targetX`**, e **não** `targetX - posX`)! Quando o volante está à direita do centro (`posX > 0`, `rawX > 32768`), `err > 0` gera força **positiva** (`+force`) para puxá-lo à esquerda de volta ao centro (`32768`); quando está à esquerda (`posX < 0`, `rawX < 32768`), `err < 0` gera força **negativa** (`-force`) para puxá-lo à direita de volta ao centro (`32768`). Inverter esse sinal (`targetX - posX`) causa *realimentação positiva (runaway)* direto para o fim de curso mecânico!
5. **Temporização Real de `1000 Hz` (`timeBeginPeriod(1)`), Relógio Wall-Clock (`timeGetTime()`) e Deduplicação de `SetParameters` (`Hotfix 002`):**
   * **Timer Multimídia e Prioridade do Agendador (`FUN_004024d0`, `FUN_00401210` e `FUN_004078c0`):**
     * Em `FUN_004024d0` (linhas `2417–2420`), `WheelCheck.exe` executa `SetThreadAffinityMask(GetCurrentThread(), 1)` e `timeBeginPeriod(1)` (`winmm.dll`) para reduzir o quantum de timer do kernel do Windows de `15,625 ms` (`64 Hz`) para `1,0 ms` (`1000 Hz`).
     * Em `FUN_00401210` (linha `1742`), eleva o processo para `HIGH_PRIORITY_CLASS` (`0x80`), e durante o `Step Log 2` (`case 0`, linha `4996`) eleva para `REALTIME_PRIORITY_CLASS` (`0x100`), restaurando `0x80` no `case 5` (linha `5085`).
     * Em `FUN_004078c0` (linhas `4713–4719`), o passo de tempo `dt` entregue ao PID (`local_17c = _DAT_004580d8`, inicializado em `0.001f` @ `0x004580D8`) passa por um filtro exponencial EMA com `_DAT_004580d4 = 0.90f`: $\Delta t_{\text{smooth}} = 0.10 \cdot \Delta t_{\text{raw}} + 0.90 \cdot \Delta t_{\text{prev}}$. A `1000 Hz` ($\Delta t = 0.001\text{ s}$), o coeficiente do filtro derivativo do PID é $\alpha = (0.90 \times 0.001) / 0.001 = 0.90$, permitindo frenagem derivativa antecipada ($50\text{ ms}$ antes de $0^\circ$) com `param_4 = 10000` (linha `3188`) e acomodação em **`~520–580 ms`** por passo (`~58 s` para 50 passos).
   * **Medição Direta por Relógio Wall-Clock vs. Truncamento Inteiro de `Sleep(1)` (`5/3 = 1,67x–1,68x`):**
     * No Windows 10/11, mesmo com `timeBeginPeriod(1)` + `HIGH_PRIORITY_CLASS`, a chamada Win32 `Sleep(1)` aguarda a segunda borda de interrupção de `1 ms` do kernel, durando na prática **`1,667 ms` (`5/3 ms`, ou `~1,68 ms` somando `Poll` + `GetDeviceState`)** por iteração (`300` chamadas = `500,2–504 ms`).
     * Em `WheelCheck.exe` (linhas `4971–4975`), o tempo de estado **nunca** é somado por ticks truncados: ele subtrai o relógio real `local_2e8 = timeGetTime() - _DAT_0045be30`, encerrando o Estado `3` e o Estado `4` em **exatos `300 ms` reais** (~180 iterações de `1,67 ms`). Somar `Duration::as_millis()` truncado por tick descartava os `0,68 ms` fracionários a cada ciclo (`1,68 ms -> 1 ms`), mantendo o torque do Estado `4` ativo por `300 * 1,68 ms = 504 ms` em vez de `300 ms` — explicando exatamente a razão empírica de **`504 / 300 = 1,68x` (`5/3`)** observada nos ângulos do G920!
   * **Início Único `Start(INFINITE, 0)` e Deduplicação de `SetParameters` (`linhas 5931–6020`):**
     * Quando `DAT_0045bb74[0]` passa de `0` para `1` (linha `5931`), `g_pEffect[0]->Start(0xFFFFFFFF /* INFINITE */, 0)` é chamado **uma única vez**, permanecendo ativo nos estados `1..4`.
     * Na linha `5944` (`local_148 != DAT_0045bd0c`), `SetParameters(&local_a0, 0x100 /* DIEP_TYPESPECIFICPARAMS */)` **só é enviado ao barramento USB HID quando o inteiro de força realmente muda** (ex.: apenas 1 pacote USB no início do Estado `3` com `force = 0`, e 1 único pacote USB no início do Estado `4` com `force = +F_s`).

---

## 4. Auditoria Real: `DIView` (`DIView.exe.c` / `DIView v4.65.116`)

### 4.1. Mapa de Funções e Offsets Verificados

| Símbolo / Endereço VA | Linhas em `DIView.exe.c` | Responsabilidade |
| :--- | :--- | :--- |
| `FUN_004010b0` (`0x004010B0`) | `1359–1537` | Construtor de `CAxisDoc`: identifica o GUID do eixo (`GUID_XAxis` @ `0x00410bc8`, `GUID_YAxis` @ `0x00410bb8`, `GUID_ZAxis` @ `0x00410ba8`, `GUID_RxAxis` @ `0x00410b98`, `GUID_RyAxis` @ `0x00410b88`, `GUID_RzAxis` @ `0x00410b78`, `GUID_Slider` @ `0x00410b68`, `GUID_POV` @ `0x00410b48`). |
| `FUN_00401360` (`0x00401360`) | `1603–1640` | Lê o valor **Cooked (Calibrado)** do eixo a partir do buffer `device_entry + 0x2A8 + axis_offset` protegido por `CRITICAL_SECTION` (`+0x744`). |
| `FUN_004013d0` (`0x004013D0`) | `1644–1681` | Lê o valor **Raw (Bruto)** do eixo a partir do buffer `device_entry + 0x3A8 + axis_offset` protegido pela mesma `CRITICAL_SECTION` (`+0x744`). |
| `FUN_004015f0` (`0x004015F0`) | `1769–2018` | `CAxisView::OnDraw`: renderiza o ponteiro Cooked (preto) e, se `m_bShowRaw` (`this + 0xAC`) estiver ativo, renderiza o ponteiro e texto **Raw** em vermelho (`0xFF`, linhas `1922–1966`). |
| `0x00401EA0` | *(Handler MFC)* | Alterna o flag booleano `this->m_bShowRaw` (`*(int*)(this + 0xAC) = (*(int*)(this + 0xAC) == 0)`), que habilita/desabilita a exibição da camada Raw na view. |
| `0x00401EF0` | *(Handler MFC)* | `CAxisView::OnDeadzone`: abre `CDeadzoneDlg` (`0x0040B540`), converte `%` $\to [0, 10000]$ ($\times 100$) e chama `pCookedDevice->SetProperty(DIPROP_DEADZONE /* 5 */, &axis_obj->diprop_deadzone)`. |
| `0x004020E0` | *(Handler MFC)* | `CAxisView::OnCalibrate`: abre `CCalibrateDlg` (`0x0040B440`), atualiza `lMin`, `lCenter`, `lMax` e chama `pCookedDevice->SetProperty(DIPROP_CALIBRATION /* 0x0B */, &axis_obj->dipropcal)`. |
| `0x00402250` | *(Handler MFC)* | `CAxisView::OnSaturation`: abre `CSaturationDlg` (`0x0040B5E0`), converte `%` $\to [0, 10000]$ ($\times 100$) e chama `pCookedDevice->SetProperty(DIPROP_SATURATION /* 6 */, &axis_obj->diprop_saturation)`. |
| `0x004041F0` | *(Init COM)* | Chama `CoInitialize(NULL)`, `CoCreateInstance(&CLSID_DirectInput8, NULL, 1, &IID_IDirectInput8A, &this->pDI)` e `pDI->Initialize(hInst, 0x0800)`. |
| `FUN_004043d0` (`0x004043D0`) | `3199–3295` | Configura o par de dispositivos (`param_1[0]` = Cooked, `param_1[1]` = Raw) com `SetCooperativeLevel(NULL, 10 /* DISCL_NONEXCLUSIVE | DISCL_BACKGROUND */)`, `SetDataFormat`, `DIPROP_BUFFERSIZE`, `Acquire` e faz a leitura inicial `GetDeviceState` para `param_1 + 0xAA` (`+0x2A8`) e `param_1 + 0xEA` (`+0x3A8`). |
| `FUN_00404560` (`0x00404560`) | `3299–3480` | Callback de `EnumDevices`: cria **duas** instâncias `IDirectInputDevice8A` por controlador (`CreateDevice` nas linhas `3378` e `3380`), enumera objetos (`0x00404DC0`), associa eventos (`SetEventNotification`, vtable `+0x30`) e inicia a thread de leitura `0x00404C20`. |
| `0x00404DC0` | *(Callback)* | `EnumObjectsCallback`: lê todas as propriedades do eixo via `GetProperty` (`vtable + 0x14`) na instância Cooked e configura `DIPROP_CALIBRATIONMODE = 1 (RAW)` via `SetProperty` (`vtable + 0x18`) na instância Raw (`0x00405058`). |
| `FUN_004054f0` (`0x004054F0`) | `3743–3829` | Consulta `GetProperty(DIPROP_GUIDANDPATH /* 0x0C */)` e usa `HidD_GetPreparsedData` + `HidP_GetSpecificValueCaps` (`FUN_0040b680`–`FUN_0040b800`) para extrair `LogicalMin` e `LogicalMax` reais do descritor USB HID. |
| `FUN_0040b490` (`0x0040B490`) | `8653–8666` | `CCalibrateDlg::DoDataExchange`: vincula os campos de edição `0x3F1` (`lMax`, `+0x60`), `0x3F3` (`lMin`, `+0x64`) e `0x3EF` (`lCenter`, `+0x68`), validando os limites `[LogicalMin, LogicalMax]`. |

### 4.2. Tabela Oficial de Constantes `DIPROP_*` e Offsets de Vtable (`IDirectInputDevice8`)

#### Offsets na Vtable de `IDirectInputDevice8A` / `IDirectInputDevice8W`:
* `+0x08` (`2`): `Release`
* `+0x0C` (`3`): `GetCapabilities`
* `+0x10` (`4`): `EnumObjects`
* `+0x14` (`5`): `GetProperty(REFGUID rguidProp, LPDIPROPHEADER pdiph)`
* `+0x18` (`6`): `SetProperty(REFGUID rguidProp, LPCDIPROPHEADER pdiph)`
* `+0x1C` (`7`): `Acquire`
* `+0x20` (`8`): `Unacquire`
* `+0x24` (`9`): `GetDeviceState(DWORD cbData, LPVOID lpvData)`
* `+0x28` (`10`): `GetDeviceData`
* `+0x2C` (`11`): `SetDataFormat`
* `+0x30` (`12`): `SetEventNotification`
* `+0x34` (`13`): `SetCooperativeLevel(HWND hwnd, DWORD dwFlags)`
* `+0x38` (`14`): `GetObjectInfo`
* `+0x3C` (`15`): `GetDeviceInfo`
* `+0x48` (`18`): `CreateEffect`
* `+0x58` (`22`): `SendForceFeedbackCommand`
* `+0x64` (`25`, decimal `100`): `Poll`

#### Pseudo-GUIDs de Propriedades (`MAKEDIPROP` — Passados como Inteiros no 2º Argumento de `GetProperty`/`SetProperty`):
| Constante DirectInput | Valor Hex (Dec) | Struct C / Rust | Tamanho Total (`dwSize`) | Verificado em |
| :--- | :--- | :--- | :--- | :--- |
| `DIPROP_BUFFERSIZE` | `0x01` (`1`) | `DIPROPDWORD` | `0x14` (`20 B`) | `DIView.exe:3258`, `WheelCheck.exe:2771` |
| `DIPROP_AXISMODE` | `0x02` (`2`) | `DIPROPDWORD` | `0x14` (`20 B`) | `WheelCheck.exe:4309` |
| `DIPROP_GRANULARITY` | `0x03` (`3`) | `DIPROPDWORD` | `0x14` (`20 B`) | `WheelCheck.exe:3964` |
| `DIPROP_RANGE` | `0x04` (`4`) | `DIPROPRANGE` | `0x18` (`24 B`) | `DIView.exe:0x404E43`, `WheelCheck.exe:2796` |
| `DIPROP_DEADZONE` | `0x05` (`5`) | `DIPROPDWORD` | `0x14` (`20 B`) | `DIView.exe:0x401F58`, `WheelCheck.exe:3944` |
| `DIPROP_SATURATION` | `0x06` (`6`) | `DIPROPDWORD` | `0x14` (`20 B`) | `DIView.exe:0x4022BE`, `WheelCheck.exe:3954` |
| `DIPROP_FFGAIN` | `0x07` (`7`) | `DIPROPDWORD` | `0x14` (`20 B`) | `WheelCheck.exe:2764` |
| `DIPROP_FFLOAD` | `0x08` (`8`) | `DIPROPDWORD` | `0x14` (`20 B`) | `WheelCheck.exe:4335` |
| `DIPROP_AUTOCENTER` | `0x09` (`9`) | `DIPROPDWORD` | `0x14` (`20 B`) | `WheelCheck.exe:2757` |
| **`DIPROP_CALIBRATIONMODE`** | **`0x0A` (`10`)** | `DIPROPDWORD` | **`0x14` (`20 B`)** | `DIView.exe:0x405055`, `WheelCheck.exe:3917` |
| **`DIPROP_CALIBRATION`** | **`0x0B` (`11`)** | `DIPROPCAL` | **`0x1C` (`28 B`)** | `DIView.exe:0x40218B`, `DIView.exe:0x405014` |
| `DIPROP_GUIDANDPATH` | `0x0C` (`12`) | `DIPROPGUIDANDPATH` | `0x228` (`552 B`) | `DIView.exe:3778` |
| `DIPROP_PHYSICALRANGE` | `0x12` (`18`) | `DIPROPRANGE` | `0x18` (`24 B`) | `DIView.exe:0x404ED9`, `WheelCheck.exe:3984` |
| `DIPROP_LOGICALRANGE` | `0x13` (`19`) | `DIPROPRANGE` | `0x18` (`24 B`) | `DIView.exe:0x404E91`, `WheelCheck.exe:2781` |

### 4.3. Layout Binário Exato de `DIPROPHEADER`, `DIPROPDWORD` e `DIPROPCAL`

> **ARMADILHA CRÍTICA CORRIGIDA:**
> Na especificação teórica (Seção 2.4), `DIPROPHEADER` foi descrito com $12\text{ bytes}$ e `DIPROPCAL` com $24\text{ bytes}$.
> **Na realidade (verificado em `DIView.exe` @ `0x00405000` e `0x00402180`), `DIPROPHEADER` possui $16\text{ bytes}$ (`0x10`) e `DIPROPCAL` possui $28\text{ bytes}$ (`0x1C`)!** Passar `dwSize = 24` ou `dwHeaderSize = 12` faz `dinput8.dll` abortar imediatamente com `DIERR_INVALIDPARAM` (`0x80070057`).

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiPropHeader {
    pub dw_size: u32,        // Offset 0x00: sizeof(StructCompleta) -> 0x14 (20) p/ DWORD, 0x18 (24) p/ RANGE, 0x1C (28) p/ CAL
    pub dw_header_size: u32, // Offset 0x04: sizeof(DiPropHeader) -> SEMPRE 0x10 (16 bytes)!
    pub dw_obj: u32,         // Offset 0x08: ID do objeto (DIDEVICEOBJECTINSTANCE::dwType se DIPH_BYID, ou offset se DIPH_BYOFFSET)
    pub dw_how: u32,         // Offset 0x0C: 0 = DIPH_DEVICE, 1 = DIPH_BYOFFSET, 2 = DIPH_BYID
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiPropCal {
    pub diph: DiPropHeader,  // Offset 0x00..0x0F (16 bytes): dw_size = 28 (0x1C), dw_header_size = 16 (0x10), dw_how = 2 (DIPH_BYID)
    pub l_min: i32,          // Offset 0x10..0x13 (4 bytes LE)
    pub l_center: i32,       // Offset 0x14..0x17 (4 bytes LE)
    pub l_max: i32,          // Offset 0x18..0x1B (4 bytes LE)
}
```

### 4.4. Como o `DIView.exe` Persiste a Calibração no Registro do Windows
1. **Ausência de `RegSetValueEx` no `DIView.exe`:** A tabela de importação (IAT) do `DIView.exe` **não importa `ADVAPI32.dll`**. O `DIView.exe` nunca abre `HKCU\System\CurrentControlSet\Control\MediaProperties\PrivateProperties\DirectInput\...` manualmente!
2. **Persistência Nativa via `IDirectInputDevice8::SetProperty(DIPROP_CALIBRATION)`:**
   Em `0x00402176`–`0x00402190`, quando o usuário confirma o diálogo `CCalibrateDlg` (`IDOK`), o `DIView.exe` executa apenas:
   ```c
   // esi = CAxisDoc*, [esi + 0x70] = AxisObject*, [esi + 0x74] = DeviceEntry*
   axis_obj->dipropcal.lMin    = dlg.m_min;    // [ecx + 0x1D8]
   axis_obj->dipropcal.lCenter = dlg.m_center; // [eax + 0x1DC]
   axis_obj->dipropcal.lMax    = dlg.m_max;    // [edx + 0x1E0]
   HRESULT hr = device_entry->pCookedDevice->lpVtbl->SetProperty(
       device_entry->pCookedDevice,
       (REFGUID)0x0B, // DIPROP_CALIBRATION (11)
       &axis_obj->dipropcal.diph
   );
   ```
   A própria `dinput8.dll` do Windows intercepta `SetProperty(DIPROP_CALIBRATION)`, serializa os $12\text{ bytes}$ (`[lMin, lCenter, lMax]`) na chave `Calibration` (`REG_BINARY`) do Registro em `HKCU\System\CurrentControlSet\Control\MediaProperties\PrivateProperties\DirectInput\VID_%04X&PID_%04X\Calibration\0\Type\Axes\%d` **e** recalibra instantaneamente todas as instâncias DirectInput abertas!
   *(No `speed-precise-core` em Rust, continuaremos realizando o backup JSON explícito da chave do Registro antes de aplicar `SetProperty(DIPROP_CALIBRATION)` + gravação no Registro, garantindo segurança e reversibilidade de 100%).*

### 4.5. Cálculo do Valor Raw Normalizado na View (`FUN_00402210`, Linhas 2104–2115 e 1938–1947)
Para posicionar o indicador visual da leitura Raw dentro da barra do eixo, o `DIView.exe` usa a função de interpolação linear `FUN_00402210`:
```c
float10 FUN_00402210(float logical_min, float logical_max, float range_min, float range_max, float raw_val)
{
    float10 offset = (logical_min < 0.0f) ? -(float10)logical_min : 0.0;
    return (range_max - range_min) * ((offset + raw_val) / (offset + logical_max)) + range_min;
}
```
E no botão **"Reset"** do diálogo de calibração (`0x0040B510`), os valores padrão são restaurados exatamente como:
```c
dlg->m_min    = dlg->m_logical_min;                              // [ecx + 0x64]
dlg->m_max    = dlg->m_logical_max;                              // [ecx + 0x60]
dlg->m_center = dlg->m_logical_min + (dlg->m_logical_max - dlg->m_logical_min + 1) / 2; // [ecx + 0x68]
```
*(Note o `+ 1` antes da divisão inteira por 2 em `0x0040B521`: `sub eax, esi; inc eax; cdq; sub eax, edx; sar eax, 1; add eax, esi` — para `0..65535`, `(65535 - 0 + 1) / 2 = 32768`!).*
