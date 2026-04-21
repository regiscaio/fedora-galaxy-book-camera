<p align="center">
  <img src="assets/galaxybook-camera.svg" alt="Ícono de Galaxy Book Camera" width="112">
</p>

<h1 align="center">Galaxy Book Camera</h1>

<p align="center">
  <a href="README.md">🇧🇷 Português</a> 
  <a href="README.en.md">🇺🇸 English</a> 
  <a href="README.es.md">🇪🇸 Español</a> 
  <a href="README.it.md">🇮🇹 Italiano</a>
</p>

## Instalación rápida

Para instalar la aplicación desde el repositorio público de DNF:

```bash
sudo dnf config-manager addrepo --from-repofile=https://packages.caioregis.com/fedora/caioregis.repo
sudo dnf install galaxybook-camera akmod-galaxybook-ov02c10
```

Si también quieres el asistente gráfico de instalación y diagnóstico:

```bash
sudo dnf install galaxybook-setup
```

`Galaxy Book Camera` es una aplicación de cámara para Fedora en portátiles
Samsung Galaxy Book, con foco actual en el **Galaxy Book4 Ultra**. Habla
directamente con `libcamera`, usa una interfaz nativa de GNOME con `GTK4` y
`libadwaita`, y fue diseñada para funcionar junto con el driver empaquetado en
[`fedora-galaxy-book-ov02c10`](https://github.com/regiscaio/fedora-galaxy-book-ov02c10).

Este repositorio cubre solo la **parte de userspace**: interfaz, captura de
fotos, grabación de vídeo y controles de imagen. El módulo del kernel vive en
otro repositorio.

## Estado actual

Según el estado validado en abril de 2026, la aplicación ya cubre el flujo
principal de cámara en el Galaxy Book4 Ultra:

- vista previa directa vía `libcamera`;
- foto y vídeo con UI nativa de GNOME;
- zoom expuesto en el dock principal;
- tuning dedicado del sensor `ov02c10` para reducir el fallback totalmente
  `uncalibrated` de `libcamera`;
- integración con el driver `ov02c10` empaquetado y con `Galaxy Book Setup`.

El flujo para navegador, Meet, Discord, Teams y otras apps WebRTC sigue
dependiendo de la configuración del sistema, por eso esa parte permanece
documentada y automatizada en `Galaxy Book Setup`.

En el stack actual, la aplicación nativa de cámara de Fedora/GNOME ya puede
funcionar en este portátil. Aun así, ambos caminos no producen exactamente el
mismo resultado visual:

- la app nativa de Fedora suele mostrar una imagen más procesada, con color y
  balance de blancos más agradables por defecto;
- `Galaxy Book Camera` usa una ruta directa vía `libcamera`, visualmente más
  cruda y más cercana al sensor, preservando más detalle fino y ofreciendo un
  control mucho más flexible sobre la imagen.

## Por qué existe una app dedicada

La aplicación nativa de cámara de GNOME fue una referencia importante de
interfaz e integración con el escritorio, pero no resuelve por sí sola el caso
específico del Galaxy Book4 Ultra.

En este hardware, la webcam depende de una combinación más delicada de:

- un driver `ov02c10` fuera del camino in-tree puro del kernel;
- el stack Intel IPU6;
- `libcamera`;
- un bridge para navegador y comunicadores cuando hace falta.

En la práctica, el camino genérico del escritorio no siempre era el mejor sitio
para validar el sensor, la vista previa y los ajustes específicos de este
equipo. `Galaxy Book Camera` existe justamente para:

- hablar directamente con `libcamera` en el flujo principal;
- cargar tuning propio del sensor `ov02c10`;
- exponer los controles que tenían sentido para este hardware;
- priorizar detalle y control fino de captura en vez de depender solo del
  procesamiento genérico del escritorio;
- separar el uso diario de la cámara del flujo de reparación, diagnóstico y
  bridge, que quedó concentrado en `Galaxy Book Setup`.

## Alcance

El proyecto ofrece:

- vista previa embebida en la ventana principal;
- selector de zoom en el dock principal con niveles `1x`, `2x`, `3x`, `5x` y `10x`;
- captura de fotos en la mayor resolución still expuesta por la cámara;
- grabación de vídeo con audio opcional;
- tuning dedicado `ov02c10.yaml` para la ruta directa de `libcamera`;
- cuenta atrás de `3s`, `5s` o `10s` para foto y vídeo;
- preferencias persistentes de imagen y comportamiento;
- postprocesado calibrado para reducir dominantes verdes y azules más fuertes;
- diálogo `Acerca de` nativo de `libadwaita`, con enlaces y página de detalles;
- integración con `.desktop`, icono propio y ventana nativa de GNOME;
- controles de brillo, exposición, contraste, saturación, matiz, temperatura,
  tinte, RGB, gamma, nitidez y espejo.

Este proyecto **no** ofrece:

- el parche del módulo `ov02c10`;
- bridge de webcam virtual para apps que dependen estrictamente de V4L2;
- correcciones específicas del stack `PipeWire`/`xdg-desktop-portal` del host.

## Requisitos en runtime

Para funcionar en este hardware, el sistema necesita:

- `libcamera`;
- `GTK4` y `libadwaita`;
- `ffmpeg-free` de Fedora o `ffmpeg` de RPM Fusion;
- el driver empaquetado en `fedora-galaxy-book-ov02c10`.

## Instalación para usuarios

### Vía repositorio público de DNF

La ruta recomendada es:

```bash
sudo dnf config-manager addrepo --from-repofile=https://packages.caioregis.com/fedora/caioregis.repo
sudo dnf install galaxybook-camera akmod-galaxybook-ov02c10
```

Si también quieres los flujos guiados de reparación, diagnóstico y webcam para
navegador:

```bash
sudo dnf install galaxybook-setup
```

### Vía RPM local

Si los RPM fueron generados localmente, instala primero los paquetes del driver
y luego la app:

```bash
sudo dnf install \
  /ruta/a/galaxybook-ov02c10-kmod-common-*.rpm \
  /ruta/a/akmod-galaxybook-ov02c10-*.rpm \
  /ruta/a/galaxybook-camera-*.rpm
sudo reboot
```

Si la cámara sigue fallando después del reinicio, los checks más útiles son:

```bash
journalctl -b -u akmods --no-pager
modinfo -n ov02c10
journalctl -b -k | grep -i ov02c10
```

## Uso

Después de la instalación y del reinicio, la aplicación se puede abrir desde el
menú de GNOME.

Comportamiento actual:

- las fotos se guardan en `XDG_PICTURES_DIR/Camera`;
- los vídeos se guardan en `XDG_VIDEOS_DIR/Camera`;
- la cámara se abre directamente vía `libcamera`, sin depender de Snapshot;
- la app inyecta su propio tuning file `ov02c10` en el simple IPA de
  `libcamera`;
- el preset `Natural` y el baseline por defecto usan una calibración ligera
  para acercar el color al camino de webcam del sistema sin perder detalle;
- el postprocesado neutraliza parte de las dominantes verdes y azules más
  agresivas en sombras profundas e iluminación difícil;
- el zoom de la vista previa usa un selector inline en el dock principal.

## Limitaciones conocidas

- Este repositorio está enfocado en la app de cámara con UI nativa de GNOME.
  La visibilidad de la cámara en Snapshot, navegadores, Meet, Teams o Discord
  depende del stack del sistema (`PipeWire`, `WirePlumber`, `libcamera`,
  `xdg-desktop-portal`) y no se resuelve solo con este paquete.
- El soporte fue trabajado y validado principalmente en el **Galaxy Book4 Ultra**.

## Relación con el driver y con el fix comunitario

El módulo del kernel usado por esta app vive en:

- <https://github.com/regiscaio/fedora-galaxy-book-ov02c10>

Ese trabajo parte de las lecciones del repositorio comunitario:

- <https://github.com/abdallah-alkanani/galaxybook3-ov02c10-fix/>

## Build y empaquetado

Dependencias de build en Fedora:

```bash
sudo dnf install cargo rust pkgconf-pkg-config gtk4-devel libadwaita-devel libcamera-devel
```

Comandos principales:

```bash
make build
make test
make dist
make srpm
make rpm
```

## Licencia

Este proyecto se distribuye bajo **GPL-3.0-only**. Consulta [LICENSE](LICENSE).
