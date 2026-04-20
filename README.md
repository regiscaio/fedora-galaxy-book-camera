<p align="center">
  <img src="assets/galaxybook-camera.svg" alt="Ícone do Galaxy Book Camera" width="112">
</p>

<h1 align="center">Galaxy Book Camera</h1>

<p align="center">
  <a href="README.md">🇧🇷 Português</a> ·
  <a href="README.en.md">🇺🇸 English</a> ·
  <a href="README.es.md">🇪🇸 Español</a> ·
  <a href="README.it.md">🇮🇹 Italiano</a>
</p>

## Instalação rápida

Para instalar o app a partir do repositório DNF público:

```bash
sudo dnf config-manager addrepo --from-repofile=https://packages.caioregis.com/fedora/caioregis.repo
sudo dnf install galaxybook-camera akmod-galaxybook-ov02c10
```

Se você também quiser o auxiliar gráfico de instalação e diagnóstico:

```bash
sudo dnf install galaxybook-setup
```

`Galaxy Book Camera` é um app de câmera para Fedora em notebooks Samsung
Galaxy Book, com foco atual no **Galaxy Book4 Ultra**. O app usa `libcamera`
diretamente, tem UI nativa do GNOME com `GTK4` e `libadwaita`, e foi pensado
para funcionar junto do driver empacotado em
[`fedora-galaxy-book-ov02c10`](https://github.com/regiscaio/fedora-galaxy-book-ov02c10).

Este repositório cobre apenas o **lado userspace**: interface, captura de
foto, gravação de vídeo e ajustes de imagem. O módulo do kernel fica em um
repositório separado.

## Status atual

No estado validado em abril de 2026, o app já cobre o fluxo principal de câmera
no Galaxy Book4 Ultra:

- preview direto via `libcamera`;
- foto e vídeo com UI nativa do GNOME;
- zoom exposto no dock principal;
- tuning dedicado do sensor `ov02c10` para reduzir o fallback totalmente
  `uncalibrated` do `libcamera`;
- integração com o driver `ov02c10` empacotado e com o `Galaxy Book Setup`.

O fluxo para navegador, Meet, Discord, Teams e outros apps WebRTC continua
dependendo do setup do host, por isso esse caminho segue documentado e
automatizado no `Galaxy Book Setup`.

No estado atual do stack, o app nativo de câmera do Fedora/GNOME já pode
funcionar neste notebook. Ainda assim, os dois caminhos não entregam exatamente
o mesmo resultado visual:

- o app nativo do Fedora tende a mostrar uma imagem mais processada, com cor,
  balanço de branco e aparência geral mais “prontos”;
- o `Galaxy Book Camera` usa um caminho direto via `libcamera`, visualmente
  mais cru e mais próximo do sensor, preservando mais detalhe fino e
  oferecendo controle muito mais flexível sobre a imagem.

Na prática, o app nativo pode parecer melhor em cor no estado padrão, enquanto
o `Galaxy Book Camera` tende a entregar melhor detalhe e mais margem de ajuste.

## Por que existe um app dedicado

O app nativo de câmera do GNOME foi uma referência importante de interface e de
integração com o desktop, mas ele não resolve sozinho o caso específico do
Galaxy Book4 Ultra.

Neste hardware, a webcam depende de uma combinação mais sensível entre:

- driver `ov02c10` fora do caminho in-tree puro do kernel;
- stack Intel IPU6;
- `libcamera`;
- bridge para navegador e comunicadores quando necessário.

Na prática, o caminho genérico do desktop nem sempre era o melhor lugar para
validar o sensor, o preview e os ajustes específicos desse notebook. O
`Galaxy Book Camera` existe justamente para:

- falar diretamente com o `libcamera` no fluxo principal da câmera;
- carregar tuning próprio do sensor `ov02c10`;
- expor a interface e os controles que fizeram sentido para esse hardware;
- priorizar detalhe e controle fino da captura, em vez de depender apenas do
  processamento padrão do caminho genérico do desktop;
- separar o uso diário da câmera do fluxo de reparo, diagnóstico e bridge,
  que ficou concentrado no `Galaxy Book Setup`.

Em outras palavras: o objetivo não foi “substituir o app nativo do Fedora por
gosto”, e sim criar um caminho estável e controlável para um hardware que
precisou de solução dedicada.

## Escopo

O projeto entrega:

- preview embutido na janela principal;
- seletor de zoom no dock principal, com níveis `1x`, `2x`, `3x`, `5x` e `10x`;
- captura de foto na maior resolução still exposta pela câmera;
- gravação de vídeo com áudio opcional;
- tuning dedicado `ov02c10.yaml` para o caminho direto do `libcamera`;
- contagem regressiva de `3s`, `5s` ou `10s` para foto e início de vídeo;
- preferências persistidas para imagem e comportamento;
- pós-processamento calibrado para reduzir casts verdes e azulados mais
  agressivos em sombras profundas e extremos de luz;
- modal `Sobre` nativa em `libadwaita`, com links e seção `Detalhes`;
- integração com launcher `.desktop`, ícone próprio e janela nativa do GNOME;
- ajustes como brilho, exposição, contraste, saturação, matiz, temperatura,
  tinta, RGB, gamma, nitidez e espelhamento.

Este projeto **não** entrega:

- o patch do módulo `ov02c10`;
- bridge de webcam virtual para apps que dependem estritamente de V4L2;
- correções específicas do stack `PipeWire`/`xdg-desktop-portal` do host.

## Requisitos em runtime

Para o app funcionar neste hardware, o sistema precisa ter:

- `libcamera`;
- `GTK4` e `libadwaita`;
- `ffmpeg-free` do Fedora ou `ffmpeg` do RPM Fusion;
- o driver empacotado em `fedora-galaxy-book-ov02c10`.

Na prática, a instalação mais segura para usuários Fedora é instalar o conjunto
do driver com `akmod`:

- `galaxybook-ov02c10-kmod-common`
- `akmod-galaxybook-ov02c10`

## Instalação para usuários

### Via repositório DNF público

O caminho recomendado para usuários finais é instalar pelo repositório público:

```bash
sudo dnf config-manager addrepo --from-repofile=https://packages.caioregis.com/fedora/caioregis.repo
sudo dnf install galaxybook-camera akmod-galaxybook-ov02c10
```

Se você também quiser o fluxo assistido de reparo, validação e webcam para
navegador:

```bash
sudo dnf install galaxybook-setup
```

### Via RPM local

Se os RPMs foram gerados localmente, instale primeiro os pacotes do driver e,
na sequência, o app:

```bash
sudo dnf install \
  /caminho/para/galaxybook-ov02c10-kmod-common-*.rpm \
  /caminho/para/akmod-galaxybook-ov02c10-*.rpm \
  /caminho/para/galaxybook-camera-*.rpm
sudo reboot
```

Na primeira inicialização depois da instalação, o `akmods` deve compilar e
instalar o módulo do kernel automaticamente. Se `Secure Boot` estiver
habilitado, o fluxo de assinatura de módulos via `akmods` precisa estar
configurado corretamente no sistema. Caso contrário, o módulo pode ser
compilado, mas não carregado no boot.

Se a câmera ainda falhar depois do reboot, os checks mais úteis são:

```bash
journalctl -b -u akmods --no-pager
modinfo -n ov02c10
journalctl -b -k | grep -i ov02c10
```

O resultado esperado é o `ov02c10` vindo do módulo gerado pelo `akmods`, não
da cópia in-tree do kernel.

## Uso

Depois da instalação e do reboot, o app pode ser aberto pelo menu do GNOME com
o nome **Galaxy Book Câmera** quando o sistema estiver em `pt_BR`.

Comportamento atual:

- fotos são salvas em `XDG_PICTURES_DIR/Camera`;
- vídeos são salvos em `XDG_VIDEOS_DIR/Camera`;
- a câmera é acessada diretamente via `libcamera`, sem depender do Snapshot;
- o app injeta um tuning file próprio para o sensor `ov02c10` no `simple IPA`
  do `libcamera`, para evitar o fallback totalmente `uncalibrated`;
- o preset `Natural` e o baseline padrão usam um ajuste leve e calibrado para
  aproximar a cor do caminho de webcam do sistema sem perder o detalhe do
  `libcamera` direto;
- o pós-processamento do preview e da captura neutraliza parte dos casts verdes
  e azulados mais agressivos em sombras profundas e extremos de luz, sem
  abandonar o caráter mais cru do pipeline direto;
- o zoom do preview usa um seletor inline no dock principal, mantendo o app
  mais próximo da lógica de câmera mobile sem abandonar o layout GNOME.

## Limitações conhecidas

- O foco deste repositório é o app de câmera com UI nativa do GNOME. A
  visibilidade da câmera em apps como Snapshot, navegadores, Meet, Teams ou
  Discord depende do stack do host (`PipeWire`, `WirePlumber`, `libcamera`,
  `xdg-desktop-portal`) e não é resolvida apenas por este pacote. Para esse
  cenário, o fluxo recomendado é usar o `Galaxy Book Setup`.
- O suporte foi trabalhado e validado principalmente no **Galaxy Book4 Ultra**.
  Outros modelos da linha Galaxy Book podem exigir ajustes adicionais no
  driver, no ACPI ou no pipeline de câmera.

## Relação com o driver e com o fix comunitário

O módulo do kernel usado por este app vive em:

- <https://github.com/regiscaio/fedora-galaxy-book-ov02c10>

O trabalho nesse driver parte dos aprendizados do repositório comunitário:

- <https://github.com/abdallah-alkanani/galaxybook3-ov02c10-fix/>

A separação atual entre repositórios existe para manter responsabilidades
claras:

- `fedora-galaxy-book-ov02c10`: módulo do kernel e empacotamento `akmod`;
- `fedora-galaxy-book-camera`: app GNOME e empacotamento RPM do userspace.

## Build e empacotamento

Dependências de build no Fedora:

```bash
sudo dnf install cargo rust pkgconf-pkg-config gtk4-devel libadwaita-devel libcamera-devel
```

Se o host não tiver o toolchain completo, o `Makefile` usa um container rootless
com `podman`.

Comandos principais:

```bash
make build
make test
make dist
make srpm
make rpm
```

O binário gerado localmente fica em:

```bash
./target/release/galaxybook-camera
```

O launcher local de desenvolvimento pode ser instalado com:

```bash
make install-local
```

Arquivos relevantes:

- spec RPM: [`packaging/fedora/galaxybook-camera.spec`](packaging/fedora/galaxybook-camera.spec)
- launcher: [`data/com.caioregis.GalaxyBookCamera.desktop`](data/com.caioregis.GalaxyBookCamera.desktop)
- metadados AppStream: [`data/com.caioregis.GalaxyBookCamera.metainfo.xml`](data/com.caioregis.GalaxyBookCamera.metainfo.xml)

## Licença

Este projeto é distribuído sob a licença **GPL-2.0-only**. Veja o arquivo
[LICENSE](LICENSE).
