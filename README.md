# Galaxy Book Camera

`Galaxy Book Camera` é um aplicativo de câmera nativo para Fedora em notebooks
Samsung Galaxy Book, com foco atual no **Galaxy Book4 Ultra**. O app usa
`libcamera` diretamente, tem interface GNOME com `GTK4` e `libadwaita`, e foi
pensado para funcionar junto do driver empacotado em
[`fedora-galaxy-book-ov02c10`](https://github.com/regiscaio/fedora-galaxy-book-ov02c10).

Este repositório cobre apenas o **lado userspace**: interface, captura de
foto, gravação de vídeo e ajustes de imagem. O módulo do kernel fica em um
repositório separado.

## Status atual

No estado validado em abril de 2026, o app já cobre o fluxo principal de câmera
no Galaxy Book4 Ultra:

- preview direto via `libcamera`;
- foto e vídeo com interface GNOME nativa;
- zoom exposto no dock principal;
- tuning dedicado do sensor `ov02c10` para reduzir o fallback totalmente
  `uncalibrated` do `libcamera`;
- integração com o driver `ov02c10` empacotado e com o `Galaxy Book Setup`.

O fluxo para navegador, Meet, Discord, Teams e outros apps WebRTC continua
dependendo do setup do host, por isso esse caminho segue documentado e
automatizado no `Galaxy Book Setup`.

## Escopo

O projeto entrega:

- preview embutido na janela principal;
- seletor de zoom no dock principal, com níveis `1x`, `2x`, `3x`, `5x` e `10x`, aberto diretamente do controle `1x`;
- captura de foto na maior resolução still exposta pela câmera;
- gravação de vídeo com áudio opcional;
- tuning dedicado `ov02c10.yaml` para o caminho direto do `libcamera`;
- contagem regressiva de `3s`, `5s` ou `10s` para foto e início de vídeo;
- preferências persistidas para imagem e comportamento;
- zoom do preview exposto na HUD principal, enquanto as preferências ficam focadas em preset, imagem e comportamento;
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

O RPM do app declara dependência em:

- `akmod-galaxybook-ov02c10 >= 0.1.0`

Na prática, a instalação mais segura para usuários Fedora é instalar o conjunto
do driver com `akmod`:

- `galaxybook-ov02c10-kmod-common`
- `akmod-galaxybook-ov02c10`

## Instalação para usuários

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

### Fluxo recomendado para usuários finais

O caminho mais simples hoje é:

1. instalar o driver `ov02c10`;
2. instalar o `Galaxy Book Câmera`;
3. usar o `Galaxy Book Setup` para diagnóstico, reparo do stack e exposição da webcam para navegador quando necessário.

Isso é importante porque o app de câmera resolve o uso direto via `libcamera`,
mas o fluxo para navegador, Meet, Discord, Teams e outros apps WebRTC fica
centralizado no repositório de setup.

### Via COPR

O projeto foi estruturado para distribuição por **RPM/COPR**. Quando houver um
repositório COPR publicado, a recomendação é usar esse canal para instalar e
atualizar os pacotes.

## Uso

Depois da instalação e do reboot, o app pode ser aberto pelo menu do GNOME com
o nome **Galaxy Book Câmera** quando o sistema estiver em `pt_BR`.

Comportamento atual:

- fotos são salvas em `XDG_PICTURES_DIR/Camera`;
- vídeos são salvos em `XDG_VIDEOS_DIR/Camera`;
- a câmera é acessada diretamente via `libcamera`, sem depender do Snapshot.
- o app injeta um tuning file próprio para o sensor `ov02c10` no `simple IPA`
  do `libcamera`, para evitar o fallback totalmente `uncalibrated`;
- o preset `Natural` e o baseline padrão usam um ajuste leve e calibrado para
  aproximar a cor do caminho de webcam do sistema sem perder o detalhe do
  `libcamera` direto.
- o zoom do preview usa um seletor inline no dock principal, mantendo o app
  mais próximo da lógica de câmera mobile sem abandonar o layout GNOME.

## Limitações conhecidas

- O foco deste repositório é o app nativo de câmera. A visibilidade da câmera
  em apps como Snapshot, navegadores, Meet, Teams ou Discord depende do stack
  do host (`PipeWire`, `WirePlumber`, `libcamera`, `xdg-desktop-portal`) e não
  é resolvida apenas por este pacote. Para esse cenário, o fluxo recomendado é
  usar o `Galaxy Book Setup`.
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
