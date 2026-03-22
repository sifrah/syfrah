Projet : une plateforme open source pour transformer des serveurs dédiés en cloud provider

Vision

L’idée du projet est de créer une plateforme permettant de transformer simplement un ensemble de serveurs dédiés, loués chez différents hébergeurs, en un véritable mini cloud provider opérable, multi-zones, multi-tenant et administrable comme un cloud moderne.

Aujourd’hui, le cloud public est extrêmement pratique, mais il est aussi devenu très coûteux. Pour beaucoup d’entreprises, de startups, de MSP, d’hébergeurs ou d’équipes techniques avancées, la facture liée aux hyperscalers devient difficile à justifier, surtout pour des charges de travail relativement classiques : machines virtuelles, bases de données, load balancers, réseaux privés, environnements multi-projets.

En parallèle, les serveurs dédiés restent très compétitifs en prix, en capacité et en performance. Le problème est qu’ils ne fournissent pas, à eux seuls, l’expérience d’un cloud provider. Ils donnent de la puissance brute, mais pas l’abstraction, l’orchestration, le multi-tenant, les VPC, les subnets, les règles réseau, l’IAM, les notions de régions ou de zones, ni l’expérience utilisateur attendue par des équipes modernes.

Le projet vise donc à combler cet écart.

L’ambition est de proposer un moteur open source capable de prendre des serveurs dédiés dispersés chez plusieurs providers, par exemple OVHcloud, Scaleway, Hetzner ou d’autres, puis de les relier, les organiser et les exposer comme une plateforme cloud cohérente, avec des concepts familiers : régions, availability zones, VPC, subnets, peering, shared VPC, organisations, IAM, projets, VM, load balancers et bases PostgreSQL managées.

Autrement dit, l’objectif est de permettre à n’importe quel acteur technique de construire son propre cloud provider à partir de serveurs dédiés standards, sans avoir à développer lui-même toute la couche de contrôle, d’orchestration, de réseau et d’expérience utilisateur.

⸻

Problème adressé

Le marché se situe entre deux extrêmes.

D’un côté, les hyperscalers offrent une expérience cloud très riche, mais à des coûts élevés et avec une complexité tarifaire importante. De nombreuses entreprises finissent par payer très cher pour des besoins qui pourraient être couverts par une infrastructure plus simple, plus maîtrisée et plus prévisible.

De l’autre côté, les serveurs dédiés offrent un excellent rapport performance/prix, mais restent très bruts. Ils nécessitent une forte expertise pour être interconnectés, sécurisés, automatisés et exposés sous forme de services consommables par des équipes produit ou des développeurs.

Les projets open source existants sont souvent soit trop lourds, soit trop orientés datacenter traditionnel, soit insuffisamment adaptés à un modèle moderne “dédiés-first”. Beaucoup de solutions nécessitent une équipe importante, une forte expertise d’exploitation, et sont mal adaptées à un usage frugal où l’on veut simplement prendre deux à vingt serveurs dédiés et les transformer rapidement en plateforme cloud.

Il existe donc un espace produit clair pour une solution plus légère, plus pragmatique, plus moderne, pensée non pas pour recréer un hyperscaler, mais pour rendre le cloud accessible et rentable à partir d’une infrastructure dédiée existante.

⸻

Positionnement du projet

Le projet se positionne comme une couche de transformation.

Il ne remplace pas les providers de serveurs dédiés. Il s’appuie sur eux.

Il ne cherche pas non plus à reproduire l’intégralité d’AWS, GCP ou Azure. Il cherche à fournir une expérience cloud simple, cohérente et opérable sur une infrastructure beaucoup moins coûteuse.

La proposition de valeur est la suivante :

prendre des serveurs dédiés chez plusieurs providers et les transformer en un cloud multi-zones, multi-tenant, IPv6-native, avec une couche de contrôle moderne et des services simples à consommer.

Le cœur du projet repose sur l’idée qu’un serveur dédié peut devenir une brique d’un cloud, à condition qu’on lui ajoute :
	•	un runtime de virtualisation léger,
	•	une interconnexion réseau entre nœuds,
	•	une couche d’orchestration,
	•	un modèle de ressources cloud,
	•	et une expérience opérable via API, CLI ou interface web.

⸻

Architecture générale

1. Compute

La couche compute serait basée sur Firecracker, afin de fournir des microVM légères, rapides à démarrer et économes en ressources. Ce choix permet de s’éloigner de la virtualisation lourde classique et de proposer un modèle plus dense, plus frugal et plus cloud-native.

Chaque serveur dédié devient alors un nœud de calcul capable d’héberger plusieurs microVM, avec gestion du cycle de vie, des images, du stockage local et des interfaces réseau.

2. Réseau

La connectivité entre les serveurs dédiés s’appuie sur une interconnexion chiffrée, de type WireGuard, afin de créer un backbone privé entre les différents nœuds, même lorsqu’ils se trouvent chez des providers distincts.

Au-dessus de cette couche, la plateforme expose des abstractions réseau comparables à celles des grands clouds :
	•	VPC,
	•	subnets,
	•	tables de routage,
	•	sécurité réseau,
	•	VPC peering,
	•	shared VPC,
	•	segmentation multi-tenant,
	•	projets et organisations.

Le design réseau serait pensé de manière moderne, avec une approche IPv6-native pour la connectivité publique. Cela permet de simplifier drastiquement l’adressage, de réduire les coûts liés à l’IPv4, et de construire une plateforme plus propre techniquement. L’IPv4 serait traitée comme une couche de compatibilité, notamment au travers de gateways ou de proxies gérés par la plateforme SaaS.

3. Stockage

Le projet vise à fournir des volumes persistants et des mécanismes de sauvegarde adaptés à des workloads VM et PostgreSQL. La couche stockage pourrait évoluer en plusieurs étapes, avec une approche initiale pragmatique basée sur du stockage local, puis des fonctionnalités avancées de snapshot, sauvegarde et réplication sur des stockages objets compatibles S3.

L’intention est de rester frugal et de tirer parti des stockages objets déjà proposés par les hébergeurs, tout en construisant une couche d’abstraction orientée volumes, sauvegardes et restauration.

4. Control plane

Le vrai cœur du projet se situe dans le control plane.

C’est lui qui permet de transformer un ensemble de machines hétérogènes en une plateforme cohérente. Il gère :
	•	l’inventaire des serveurs,
	•	leur rattachement à des régions et zones logiques,
	•	les projets et organisations,
	•	le placement des VM,
	•	les réseaux privés,
	•	les load balancers,
	•	les bases PostgreSQL,
	•	les politiques d’accès,
	•	et plus largement l’état global du cloud.

Ce control plane constitue la couche qui “cloudifie” des serveurs dédiés.

⸻

Produits exposés

L’approche produit serait volontairement simple au départ, afin de rester crédible et exécutable.

VM

Le premier produit est la machine virtuelle. C’est la ressource fondamentale du système. Un utilisateur doit pouvoir créer, démarrer, arrêter, supprimer et redéployer une VM dans une région et une zone logique, avec réseau privé, connectivité publique IPv6, volumes persistants et règles d’accès.

Load Balancer

Le deuxième produit est le load balancer. Il permet d’exposer plusieurs VM derrière une IP ou un endpoint, de répartir le trafic, d’appliquer des règles simples de routage et de constituer une base pour des architectures applicatives plus sérieuses.

PostgreSQL managé

Le troisième produit est PostgreSQL managé. L’idée n’est pas de construire immédiatement un service ultra complexe, mais de proposer une offre simple, lisible et utile, avec provisionnement, sauvegardes, restauration et intégration réseau dans les VPC des clients.

Ces trois produits suffisent à couvrir déjà beaucoup de cas d’usage réels.

⸻

Modèle open source et SaaS

Le projet serait structuré autour de deux offres complémentaires.

Offre open source

L’offre open source fournit le moteur principal. Elle permet à des utilisateurs techniques d’installer eux-mêmes la plateforme sur leurs serveurs dédiés, de gérer les nœuds, de les interconnecter, et d’opérer leur propre cloud via CLI, scripts, API et documentation.

Cette offre s’adresse à des profils avancés :
	•	équipes platform,
	•	MSP,
	•	hébergeurs,
	•	DevOps,
	•	homelabs professionnels,
	•	structures souhaitant garder le contrôle maximal.

L’open source apporte plusieurs bénéfices :
	•	transparence,
	•	auditabilité,
	•	crédibilité technique,
	•	adoption communautaire,
	•	confiance.

Offre SaaS

L’offre SaaS constitue la couche de confort et de gestion centralisée.

Elle permet à un utilisateur de connecter ses différents serveurs dédiés à une interface managée, qui se charge ensuite :
	•	du bootstrap des nœuds,
	•	de leur interconnexion,
	•	de la création logique des régions et AZ,
	•	de l’orchestration réseau,
	•	de la gestion des organisations, projets et IAM,
	•	de l’expérience utilisateur,
	•	de la compatibilité IPv4 via gateways ou proxies,
	•	et d’une partie des opérations quotidiennes.

Autrement dit, l’open source fournit le moteur, tandis que le SaaS fournit la tour de contrôle.

Cette séparation est essentielle : elle permet d’avoir un projet techniquement crédible et communautaire, tout en créant une couche monétisable forte autour de l’orchestration, de la simplicité et de la compatibilité avec les contraintes du marché réel.

⸻

Différenciation

Le projet se distingue sur plusieurs axes.

1. Dediés-first

Contrairement à beaucoup de stacks historiques conçues pour des datacenters complets ou des environnements on-prem lourds, le projet part du principe que l’utilisateur dispose de serveurs dédiés loués chez des providers existants, et veut les transformer rapidement en cloud.

2. Frugalité

Le projet n’essaie pas de reproduire toute la complexité d’un hyperscaler. Il vise la simplicité, la rentabilité et l’efficacité. L’objectif est de permettre de lancer une plateforme utile avec un nombre limité de serveurs et une petite équipe.

3. Multi-provider natif

L’utilisateur peut agréger des serveurs provenant de plusieurs hébergeurs et les organiser en régions ou zones logiques. Cela ouvre des perspectives fortes en termes de résilience, de souveraineté, de flexibilité fournisseur et de maîtrise des coûts.

4. IPv6-native

Le design assume une connectivité publique moderne et économiquement cohérente. L’IPv6 est utilisée comme fondation, tandis que l’IPv4 est repositionnée comme une couche de compatibilité premium ou gérée.

5. Simplicité produit

Le projet ne cherche pas à tout faire dès le départ. Il vise peu de produits, mais bien intégrés : VM, LB, PostgreSQL. Cette discipline produit augmente fortement les chances d’exécution et d’adoption.

⸻

Utilisateurs cibles

Le projet s’adresse à plusieurs catégories d’acteurs.

D’abord, les MSP et infogéreurs qui souhaitent monter une offre cloud ou plateforme sans bâtir leur propre stack depuis zéro.

Ensuite, les équipes DevOps / platform / SRE qui veulent une alternative moins coûteuse au cloud public pour certains workloads.

Il vise aussi les petits hébergeurs ou acteurs régionaux qui veulent enrichir leur offre avec des services cloud simples.

Enfin, il peut intéresser des startups techniques ou des éditeurs SaaS sensibles au coût d’infrastructure, à la souveraineté ou au contrôle de leur stack.

⸻

Ambition long terme

À long terme, le projet peut devenir plus qu’un simple outil technique. Il peut devenir une nouvelle manière de penser l’infrastructure cloud : non plus comme un service réservé à quelques géants mondiaux, mais comme une capacité que n’importe quel acteur technique peut déployer et opérer à partir de serveurs dédiés standards.

L’ambition n’est pas nécessairement de concurrencer frontalement AWS ou GCP sur l’ensemble du spectre. L’ambition est de rendre possible un cloud plus simple, plus léger, plus abordable, plus transparent et plus contrôlable.

C’est une vision du cloud comme couche logicielle d’orchestration au-dessus de ressources brutes accessibles, et non comme une boîte noire réservée à quelques très grands acteurs.

⸻

Résumé

Ce projet consiste à créer une plateforme open source, complétée par une offre SaaS, permettant de transformer plusieurs serveurs dédiés situés chez différents hébergeurs en un cloud provider cohérent, multi-zones, multi-tenant et simple à opérer.

La plateforme s’appuie sur Firecracker pour la virtualisation, une interconnexion privée entre nœuds pour le réseau, une architecture IPv6-native pour la connectivité publique, et un control plane moderne pour exposer des produits simples comme des VM, des load balancers et du PostgreSQL managé.

L’open source fournit le moteur et la crédibilité technique. Le SaaS fournit l’orchestration, l’interface, l’expérience opérable et les couches de compatibilité comme l’IPv4.

L’objectif final est de permettre à des équipes techniques, des MSP et des hébergeurs de construire leur propre cloud provider sur des serveurs dédiés, avec une approche frugale, moderne et réaliste.
