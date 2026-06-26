# Le problème

Distribuer une application serveur est inutilement compliqué.

Une app dépend d'un ensemble de choses installées sur la machine du développeur :

- un **runtime** (Node.js, Python, Java, Ruby…) ;
- des **librairies dynamiques système** (fichiers `.so`) ;
- des **packages** (`node_modules`, paquets pip, gems…) ;
- des **fichiers de configuration** et assets.

Quand tu donnes cette application à quelqu'un d'autre — un collègue, un serveur
de prod, un client — **ça casse**. Node n'est pas installé, ou c'est la mauvaise
version, ou une librairie système manque, ou les chemins diffèrent.

C'est le problème historique du **« ça marche chez moi »**.

## Pourquoi Docker ne suffit pas (pour ce cas)

Docker résout une partie du problème mais introduit une friction importante :

- il faut **installer Docker** (daemon root, service système) ;
- il faut **comprendre** images, registries, volumes, networking ;
- le daemon tourne **en permanence**, souvent en root ;
- c'est **lourd** pour simplement distribuer un outil ou un petit service.

Docker reste excellent pour l'orchestration, le multi-container, Kubernetes.
Mais pour *« tiens, lance ce serveur »*, c'est disproportionné.

## L'idée de xbin

Un seul fichier binaire, autonome, qui contient absolument tout ce dont l'app a
besoin, et qui se lance comme un programme normal.

```bash
chmod +x mon_app.xbin && ./mon_app.xbin
```

Zéro installation. Zéro configuration. Le fichier se suffit à lui-même.
