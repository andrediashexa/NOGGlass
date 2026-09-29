# Chamada para a comunidade — NOGGlass

> Texto de divulgação do NOGGlass para a comunidade de operadores (GTER, listas,
> fóruns, redes sociais). Duas versões: completa e curta. Sinta-se livre para
> adaptar o tom ao canal.

---

## Versão completa

## 🔭 NOGGlass — um Looking Glass multi-vendor, de verdade, livre e para a comunidade

Todo operador de rede já passou por isso: precisar olhar uma rota, um AS-PATH ou uma sessão BGP num roteador e esbarrar num Looking Glass que só atende **um** fabricante, roda em script PHP/Perl abandonado, ou despeja 300 linhas de CLI cru pra você garimpar na mão.

Foi pra resolver exatamente isso que nasceu o **NOGGlass** — um Looking Glass **read-only**, **multi-vendor** e **self-hosted**, pensado pra realidade de quem opera ISP e AS no Brasil e no mundo.

**O que ele entrega:**
- 🌐 **Multi-vendor de verdade:** Huawei VRP, Cisco IOS-XR/XE, MikroTik RouterOS (v6 e v7), Juniper, Nokia SR OS, Datacom DmOS, Arista, BIRD e FRR.
- 🕸️ **Grafo de AS-PATH interativo:** a propagação da rota desenhada em SVG (ranqueamento topológico Bellman-Ford), não texto cru — o BGP vira **insight visual** pro NOC.
- 🛡️ **RPKI em camadas:** usa o estado nativo do roteador quando existe (RTR) e cai pra RIPEstat/Routinator quando não; com cache e proteção contra cache poisoning.
- 🔒 **O roteador é sagrado:** imunidade a injeção de comando por design — o input do visitante **nunca** chega na linha de comando do roteador. Tudo é tipado, validado e mapeado pra comandos read-only. Ainda tem rate limiting, CAPTCHA HMAC e host key SSH fixada (anti-MitM).
- 🌎 **Interface em pt-BR, inglês e espanhol**, com tema dark/light e white-label (logo, wallpaper, cores).
- 📦 **Um único binário Rust** (ou container Alpine mínimo), usando **menos de 35 MB de RAM**. Sobe em minutos com Docker Compose.

**Quem está por trás:** o NOGGlass foi **idealizado e conduzido por Marcelo Gondim** ([@gondimcodes](https://github.com/gondimcodes)) — que abraçou a ideia de dar à comunidade um Looking Glass de qualidade e me convidou (**André Dias**, [@andrediashexa](https://github.com/andrediashexa)) para desenvolvê-lo com ele. É fruto de mão dupla, mas a semente é do Gondim.

O objetivo é simples: **um Looking Glass de qualidade, livre e sem custo** para a comunidade — não mais um projeto abandonado ou preso a um fornecedor. É **software livre (GPLv3)**. De graça pra usar, sempre.

🎙️ **Pra entender o "porquê"** de um Looking Glass importar pro seu provedor, dá uma olhada na palestra que apresentei no **GTER 52**: [*"Desvendando o Looking Glass e sua Importância para os Provedores"*](https://www.youtube.com/live/QFQ4zk86B44?t=1593).

**E é aqui que entra você:** o projeto está a caminho da versão 1.0 e a nossa régua é dura — um driver só é "verificado" quando rodou contra um **equipamento real** (não contra um PDF). Já validamos vários end-to-end (Huawei VRP, Cisco IOS-XE/IOS-XR, MikroTik v7, BIRD, FRR); outros ainda precisam de gente com o equipamento pra confirmar.

👉 **Use. Teste no seu AS. Aponte pro seu vendor. Abra issues. Mande PRs.** Se você roda Juniper, Nokia, Datacom ou MikroTik v6, sua ajuda pra verificar esses drivers vale ouro.

**Repositório:** https://github.com/andrediashexa/NOGGlass

Bora construir juntos o Looking Glass que a comunidade merece. 🚀

— Marcelo Gondim ([@gondimcodes](https://github.com/gondimcodes)) & André Dias ([@andrediashexa](https://github.com/andrediashexa))

---

## Versão curta (grupos / X / Telegram)

🔭 **NOGGlass** — Looking Glass multi-vendor, read-only e **livre (GPLv3)**.

**Idealizado pelo Marcelo Gondim** ([@gondimcodes](https://github.com/gondimcodes)), que me convidou (André Dias) pra desenvolver junto, com um objetivo: dar à comunidade um Looking Glass de qualidade e sem custo.

Grafo de AS-PATH visual, RPKI em camadas, "o roteador é sagrado" (zero injeção), binário Rust <35MB, i18n pt-BR/en/es. Huawei, Cisco, MikroTik, Juniper, Nokia, Datacom, Arista, BIRD, FRR.

🎙️ Por que um LG importa? Palestra no GTER 52: https://www.youtube.com/live/QFQ4zk86B44?t=1593

De graça pra usar. **Testa no teu AS, aponta pro teu vendor e contribui!** 🚀
👉 https://github.com/andrediashexa/NOGGlass
