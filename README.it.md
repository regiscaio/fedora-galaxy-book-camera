<p align="center">
  <img src="assets/galaxybook-camera.svg" alt="Icona di Galaxy Book Camera" width="112">
</p>

<h1 align="center">Galaxy Book Camera</h1>

<p align="center">
  <a href="README.md">🇧🇷 Português</a> ·
  <a href="README.en.md">🇺🇸 English</a> ·
  <a href="README.es.md">🇪🇸 Español</a> ·
  <a href="README.it.md">🇮🇹 Italiano</a>
</p>

## Installazione rapida

Per installare l'app dal repository DNF pubblico:

```bash
sudo dnf config-manager addrepo --from-repofile=https://packages.caioregis.com/fedora/caioregis.repo
sudo dnf install galaxybook-camera akmod-galaxybook-ov02c10
```

Se vuoi anche l'assistente grafico di installazione e diagnostica:

```bash
sudo dnf install galaxybook-setup
```

`Galaxy Book Camera` è un'app di fotocamera per Fedora sui notebook Samsung
Galaxy Book, con focus attuale sul **Galaxy Book4 Ultra**. Parla direttamente
con `libcamera`, usa una UI GNOME nativa con `GTK4` e `libadwaita`, ed è stata
progettata per lavorare insieme al driver pacchettizzato in
[`fedora-galaxy-book-ov02c10`](https://github.com/regiscaio/fedora-galaxy-book-ov02c10).

Questo repository copre solo la **parte userspace**: interfaccia, foto, video e
controlli dell'immagine. Il modulo kernel vive in un repository separato.

## Stato attuale

Nello stato validato ad aprile 2026, l'app copre già il flusso principale della
fotocamera sul Galaxy Book4 Ultra:

- anteprima diretta via `libcamera`;
- foto e video con UI GNOME nativa;
- zoom esposto nel dock principale;
- tuning dedicato del sensore `ov02c10` per ridurre il fallback completamente
  `uncalibrated` di `libcamera`;
- integrazione con il driver `ov02c10` pacchettizzato e con `Galaxy Book Setup`.

Il flusso per browser, Meet, Discord, Teams e altre app WebRTC dipende ancora
dalla configurazione del sistema, quindi quella parte resta documentata e
automatizzata in `Galaxy Book Setup`.

Nello stack attuale, anche l'app fotocamera nativa di Fedora/GNOME può già
funzionare su questo notebook. Tuttavia i due percorsi non producono lo stesso
risultato visivo:

- l'app nativa di Fedora tende a mostrare un'immagine più elaborata, con colore
  e bilanciamento del bianco più gradevoli di default;
- `Galaxy Book Camera` usa un percorso diretto via `libcamera`, più grezzo e
  più vicino al sensore, preservando più dettaglio fine e offrendo molto più
  controllo sull'immagine.

## Perché esiste un'app dedicata

L'app fotocamera nativa di GNOME è stata un riferimento importante per
l'interfaccia e l'integrazione desktop, ma da sola non risolve il caso del
Galaxy Book4 Ultra.

Su questo hardware la webcam dipende da una combinazione più delicata di:

- driver `ov02c10` fuori dal semplice percorso in-tree del kernel;
- stack Intel IPU6;
- `libcamera`;
- bridge per browser e comunicazione quando necessario.

In pratica, il percorso generico del desktop non era sempre il posto migliore
per validare sensore, anteprima e tuning specifico dell'hardware. `Galaxy Book
Camera` esiste proprio per:

- parlare direttamente con `libcamera` nel flusso principale;
- caricare tuning specifico del sensore `ov02c10`;
- esporre i controlli che hanno senso per questo hardware;
- privilegiare dettaglio e controllo diretto invece di dipendere solo dal
  percorso generico del desktop;
- separare l'uso quotidiano della fotocamera dai flussi di riparazione,
  diagnostica e bridge, che restano in `Galaxy Book Setup`.

## Ambito

Il progetto offre:

- anteprima incorporata nella finestra principale;
- selettore di zoom nel dock principale con livelli `1x`, `2x`, `3x`, `5x` e `10x`;
- foto alla massima risoluzione still esposta dalla camera;
- registrazione video con audio opzionale;
- tuning dedicato `ov02c10.yaml` per il percorso diretto di `libcamera`;
- conto alla rovescia di `3s`, `5s` o `10s` per foto e video;
- preferenze persistenti per immagine e comportamento;
- post-processing calibrato per ridurre dominanti verdi e blu più aggressive;
- dialogo `Informazioni` nativo di `libadwaita` con link e dettagli;
- integrazione con `.desktop`, icona dedicata e finestra GNOME nativa;
- controlli per luminosità, esposizione, contrasto, saturazione, tonalità,
  temperatura, tinta, RGB, gamma, nitidezza e specchiatura.

Questo progetto **non** offre:

- la patch del modulo `ov02c10`;
- bridge di webcam virtuale per app che dipendono strettamente da V4L2;
- correzioni specifiche del stack `PipeWire`/`xdg-desktop-portal` del sistema.

## Requisiti runtime

Per funzionare su questo hardware, il sistema deve avere:

- `libcamera`;
- `GTK4` e `libadwaita`;
- `ffmpeg-free` di Fedora oppure `ffmpeg` di RPM Fusion;
- il driver pacchettizzato in `fedora-galaxy-book-ov02c10`.

## Installazione per utenti

### Tramite il repository DNF pubblico

Il percorso consigliato è:

```bash
sudo dnf config-manager addrepo --from-repofile=https://packages.caioregis.com/fedora/caioregis.repo
sudo dnf install galaxybook-camera akmod-galaxybook-ov02c10
```

Se vuoi anche i flussi guidati di riparazione, diagnostica e webcam per il
browser:

```bash
sudo dnf install galaxybook-setup
```

### Tramite RPM locali

Se gli RPM sono stati generati localmente, installa prima i pacchetti del
driver e poi l'app:

```bash
sudo dnf install \
  /percorso/a/galaxybook-ov02c10-kmod-common-*.rpm \
  /percorso/a/akmod-galaxybook-ov02c10-*.rpm \
  /percorso/a/galaxybook-camera-*.rpm
sudo reboot
```

## Uso

Dopo installazione e riavvio, l'app può essere aperta dal menu GNOME.

Comportamento attuale:

- le foto vengono salvate in `XDG_PICTURES_DIR/Camera`;
- i video vengono salvati in `XDG_VIDEOS_DIR/Camera`;
- la camera viene aperta direttamente via `libcamera`, senza Snapshot;
- l'app inietta un proprio tuning file `ov02c10` nel simple IPA di `libcamera`;
- il preset `Natural` e il baseline predefinito usano una calibrazione leggera
  per avvicinare il colore al percorso webcam del sistema senza perdere dettaglio;
- il post-processing neutralizza parte delle dominanti verdi e blu più forti;
- lo zoom dell'anteprima usa un selettore inline nel dock principale.

## Limitazioni note

- Questo repository è focalizzato sull'app fotocamera con UI nativa di GNOME.
  La visibilità della camera in Snapshot, browser, Meet, Teams o Discord
  dipende dallo stack del sistema (`PipeWire`, `WirePlumber`, `libcamera`,
  `xdg-desktop-portal`) e non viene risolta da questo pacchetto da solo.
- Il supporto è stato sviluppato e validato principalmente sul **Galaxy Book4 Ultra**.

## Relazione con il driver e con il fix comunitario

Il modulo kernel usato da questa app vive in:

- <https://github.com/regiscaio/fedora-galaxy-book-ov02c10>

Quel lavoro parte dalle lezioni del repository comunitario:

- <https://github.com/abdallah-alkanani/galaxybook3-ov02c10-fix/>

## Build e packaging

Dipendenze di build su Fedora:

```bash
sudo dnf install cargo rust pkgconf-pkg-config gtk4-devel libadwaita-devel libcamera-devel
```

Comandi principali:

```bash
make build
make test
make dist
make srpm
make rpm
```

## Licenza

Questo progetto è distribuito con licenza **GPL-2.0-only**. Vedi [LICENSE](LICENSE).
