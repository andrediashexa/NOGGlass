<!-- lint: allow-non-english -->
<!-- lint: no-tldr -->
<!-- lint: no-diagram -->
<!--
  Imported reference material, kept verbatim in Portuguese.
  It is the infrastructure standard of a previous project by the same
  maintainers, used as the mould for this repository's Docker, Compose,
  configuration and operations layout. It is NOT a project document and is
  therefore exempt from the documentation standard.
-->

# Padrão de Infra — Hexanium

> Como a infraestrutura do Hexanium é montada (Docker, stacks, rede, segredos,
> deploy e operação), escrito para servir de **molde para um projeto novo** na
> mesma linha. Tudo aqui foi extraído do repositório como ele está hoje
> (`docker-compose.yml`, Dockerfiles, `.github/workflows`, `scripts/`, `docs/`).
>
> Onde o Hexanium tem uma dívida conhecida, o documento avisa em **⚠ No projeto
> novo** — a ideia é copiar o que deu certo, não os atalhos.

---

## 1. Visão geral

Um **monorepo** e **um único `docker-compose.yml`** sobem o sistema inteiro em
um servidor Linux. Não tem Kubernetes, nem registry privado, nem nuvem
gerenciada: é `docker compose up -d --build` em uma VM, atrás da Cloudflare.

```
            Internet
               │  HTTPS (TLS termina na Cloudflare)
          ┌────▼─────┐
          │Cloudflare│
          └────┬─────┘
               │  HTTP :80
          ┌────▼─────┐   roteia por Host + PathPrefix (labels nos containers)
          │ Traefik  │
          └┬──┬──┬──┬┘
   /       │  │  │  │ /docs, /wiki, /xavier, /cthullu, /joseph ...
 ┌─────────▼┐ │  │  └──────────────┐
 │ frontend │ │  │ /api            │
 │ Next.js  │ │ ┌▼────────┐   ┌────▼──────┐
 └──────────┘ │ │ backend │   │ agentes IA│ (FastAPI)
              │ │ Express │◄─►│ xavier... │
              │ └┬───┬────┘   └────┬──────┘
              │  │   │             │
          ┌───▼──▼┐ ┌▼──────────┐ ┌▼───────┐
          │ MySQL │ │ wa-gateway│ │ Qdrant │
          └───────┘ └┬──────────┘ └────────┘
                     │
               ┌─────▼──────┐
               │Evolution API├─► Postgres + Redis
               └────────────┘
      rede docker "default" (bridge, IPv6 habilitado) — serviços se falam por nome
```

**Princípios que guiam tudo:**

1. **Um compose, uma rede.** Os serviços se enxergam pelo nome (`http://backend:4000`,
   `http://xavier:8000`). Nada de IP fixo entre containers.
2. **Só o proxy fala com o mundo.** Banco, Redis, Evolution e agentes internos
   **não publicam porta** no host. Quem precisa de entrada pública recebe
   *labels* do Traefik.
3. **Configuração por variável de ambiente**, com default seguro no compose
   (`${VAR:-default}`) e segredo real só no `.env` (fora do git).
4. **Toda integração que age no mundo externo nasce desligada** (kill-switch
   `*_ENABLED=false` / modo *shadow*).
5. **Todo container tem teto de memória medido e comentado.**
6. **O comentário explica o porquê** — especialmente quando algo "óbvio" já quebrou.

---

## 2. Stacks

| Camada | Tecnologia | Versão / imagem | Observação |
|---|---|---|---|
| Proxy / roteamento | **Traefik** | `traefik:v2.11` | provider Docker, `exposedbydefault=false`, entrypoint `web :80` |
| Borda / TLS | **Cloudflare** | — | TLS termina na Cloudflare; origem recebe HTTP |
| Frontend | **Next.js** + React + Tailwind + shadcn/ui (Radix) | `node:20-alpine` | build de produção; `rewrites` de `/api` para o backend |
| Backend | **Node.js 20 + Express** (JavaScript, CommonJS) | `node:20-alpine` | `mysql2`, `jsonwebtoken`, `bcryptjs`, `multer`, `pdfkit`… |
| Banco principal | **MySQL 8.0** | `mysql:8.0` | schema via `init.sql` + migrations idempotentes no boot |
| Microsserviço Node | **TypeScript** (wa-gateway) | `node:20-bookworm-slim` | multi-stage, sem `node_modules` na imagem final |
| Agentes de IA | **Python 3.12 + FastAPI + Uvicorn** | `python:3.12-slim` | `httpx`, `pydantic`; LLM trocável por env |
| LLM | Ollama Cloud / Anthropic Claude / Ollama local | — | escolhido por `*_LLM_BACKEND` |
| Vetorial (RAG) | **Qdrant** | `qdrant/qdrant:v1.12.4` | embeddings com `fastembed` |
| STT | faster-whisper (local, CPU, int8) | — | modelo carregado sob demanda, descarregado por ociosidade |
| WhatsApp | **Evolution API** + Postgres 16 + Redis 7 | `evoapicloud/evolution-api` | isolada atrás do `wa-gateway` |
| Wiki | Wiki.js 2 + Postgres 15 + nginx | `ghcr.io/requarks/wiki:2` | nginx reescreve o subpath `/wiki` |
| Docs de API | Swagger UI | `swaggerapi/swagger-ui:v5.17.14` | agrega spec do Express + OpenAPI dos agentes |
| Autenticação de rede | FreeRADIUS | build próprio | portas UDP bindadas em IP específico |
| Orquestração de fluxos | n8n (externo) | — | chama webhooks do backend/agentes com `X-API-Key` |
| SSO | Microsoft Entra ID (Azure AD) | — | OAuth; mapeamento grupo → role |
| CI | GitHub Actions | — | testes unitários, E2E com Playwright, gitleaks |
| Alertas | Discord webhook | — | agentes e script de memória |

**Regra de escolha de linguagem que o projeto seguiu:**
- **Produto web (API + telas)** → Node/Express + Next.js.
- **Agente de IA / processamento de documento, áudio, PDF, OCR** → Python/FastAPI
  (o ecossistema de ML está lá).
- **Adaptador fino para um sistema de terceiros** → serviço próprio pequeno
  (ex.: `wa-gateway` isola a Evolution; o backend nunca fala com ela direto).

---

## 3. Estrutura do repositório

```
/opt/<projeto>
├── docker-compose.yml          # produção: TUDO
├── docker-compose.ci.yml       # stack mínima p/ E2E (db + backend + frontend)
├── .env.example                # versionado, sem valores reais
├── .env                        # NÃO versionado (chmod 600)
├── .gitignore
├── backend/                    # Express
│   ├── Dockerfile
│   └── src/{index.js,app.js,config.js,db.js,routes/,services/,middleware/,startup/}
├── frontend/                   # Next.js
│   ├── Dockerfile
│   └── next.config.js          # CSP + rewrites /api
├── wa-gateway/                 # TypeScript, multi-stage
├── agents/
│   └── <nome>/                 # um diretório por agente
│       ├── Dockerfile
│       ├── requirements.txt    # versões PINADAS
│       ├── docker-compose.yml  # compose standalone p/ rodar o agente isolado
│       ├── .env.example
│       ├── <nome>_system_prompt.md   # montado como volume :ro
│       ├── app/api.py          # FastAPI, expõe /health
│       └── data/               # runtime (gitignored)
├── infra/<servico>/            # serviços de infra com build próprio (freeradius)
├── db/
│   ├── init.sql                # roda só no primeiro boot do volume
│   └── backups/                # dumps (gitignored)
├── scripts/                    # bump de versão, release, re-cifragem, mem/
├── update-webhook/             # deploy disparado pela UI
├── docs/                       # runbooks e planos (markdown)
└── .github/workflows/          # test.yml, e2e.yml, secret-scan.yml
```

---

## 4. Docker — padrões de Dockerfile

### 4.1 Node (backend)

```dockerfile
FROM node:20-alpine
WORKDIR /app
COPY package*.json ./
RUN npm install --production
COPY src ./src
# a ajuda do produto viaja com o código da versão
COPY docs ./docs
EXPOSE 4000
CMD ["npm", "start"]
```

- Copia `package*.json` **antes** do código → cache de camada do `npm install`.
- Copia só o que roda (`src`, `docs`), nunca o diretório inteiro.

> **⚠ No projeto novo:** use `npm ci --omit=dev` (reprodutível pelo lockfile),
> `CMD ["node", "src/index.js"]` (sem o `npm` como PID 1, que atrapalha o SIGTERM)
> e `USER node`.

### 4.2 Next.js (frontend)

```dockerfile
FROM node:20-alpine
WORKDIR /app
COPY package*.json ./
RUN npm install
COPY next.config.js postcss.config.js tailwind.config.js ./
COPY app ./app
COPY src ./src
COPY public ./public
# NEXT_PUBLIC_* é inlined NO BUILD
ARG NEXT_PUBLIC_APP_VERSION=0.0.0
ENV NEXT_PUBLIC_APP_VERSION=$NEXT_PUBLIC_APP_VERSION
RUN npm run build
EXPOSE 5173
CMD ["npm", "run", "start"]
```

- `NEXT_PUBLIC_*` precisa chegar como **build-arg** (`build.args` no compose);
  passar só em `environment` não muda o bundle, que já foi gerado no build.
- O browser chama sempre `/api/...`; o `next.config.js` reescreve para
  `NEXT_BACKEND_INTERNAL` (`http://backend:4000`). Isso faz a stack de CI
  funcionar **sem Traefik**.
- Cabeçalhos de segurança (CSP, `X-Frame-Options`, `nosniff`, `Referrer-Policy`)
  ficam no `next.config.js`, com comentário explicando cada exceção da CSP.

> **⚠ No projeto novo:** multi-stage com `output: 'standalone'` do Next reduz a
> imagem e a memória bastante.

### 4.3 TypeScript multi-stage (wa-gateway) — **o modelo a copiar**

```dockerfile
FROM node:20-bookworm-slim AS build
WORKDIR /build
COPY package*.json ./
# devDeps são necessárias: o typescript faz o build
RUN npm ci
COPY tsconfig.json tsconfig.build.json ./
COPY src ./src
RUN npm run build

FROM node:20-bookworm-slim
WORKDIR /app
ENV NODE_ENV=production
COPY --from=build /build/dist ./dist
COPY package.json ./
USER node
EXPOSE 3001
CMD ["node", "dist/index.js"]
```

Lição registrada no compose: antes rodava `npm install && npm run dev` em
produção — 3 processos (npm → tsx watch → node), watcher no disco e **subir o
serviço dependia do registry do npm estar no ar**. Produção roda artefato
compilado, sempre.

### 4.4 Python (agentes)

```dockerfile
FROM python:3.12-slim
ENV PYTHONDONTWRITEBYTECODE=1
# log aparece na hora no `docker compose logs`
ENV PYTHONUNBUFFERED=1

# dependências de sistema só quando necessárias, limpando o cache do apt
RUN apt-get update \
    && apt-get install -y --no-install-recommends poppler-utils tesseract-ocr tesseract-ocr-por \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
COPY . .
EXPOSE 8000
CMD ["uvicorn", "app.api:app", "--host", "0.0.0.0", "--port", "8000"]
```

- `requirements.txt` com **versões exatas** (`fastapi==0.115.6`).
- Todo agente escuta na **8000** interna e expõe `GET /health`.
- Cada agente tem um `docker-compose.yml` próprio para rodar isolado em
  desenvolvimento; em produção ele entra no compose principal.

> **⚠ No projeto novo:** adicione `.dockerignore` (evita mandar `data/`,
> `.venv`, `node_modules` no contexto do build) e `USER` não-root.

---

## 5. Docker Compose — padrões de serviço

### 5.1 Esqueleto de um serviço

```yaml
  meu-servico:
    build:
      context: ./agents/meu-servico
      dockerfile: Dockerfile
    container_name: projeto-meu-servico   # só quando scripts externos usam o nome
    restart: unless-stopped
    mem_limit: 256m          # medido 72 MB
    memswap_limit: 256m      # sem swap: o teto tem que valer
    environment:
      MEU_LLM_BACKEND: ${MEU_LLM_BACKEND:-ollama_cloud}
      MEU_API_KEY: ${MEU_API_KEY:-}
      MEU_ACTIONS_ENABLED: ${MEU_ACTIONS_ENABLED:-false}   # kill-switch
      MEU_DATA_DIR: /app/data
    volumes:
      - ./agents/meu-servico/data:/app/data
      - ./agents/meu-servico/prompt.md:/app/prompt.md:ro  # editável sem rebuild
    depends_on:
      - db
    networks:
      - default
```

### 5.2 Regras

| Regra | Como aparece |
|---|---|
| **Restart** | `restart: unless-stopped` em todos |
| **Memória** | `mem_limit` = `memswap_limit` (sem swap). Comentário com o **consumo medido** e o motivo da folga (`# medido 116 MB; PDF e ZIP picam`) |
| **Portas** | Nenhum `ports:` para banco/cache/serviço interno. Quando precisa publicar fora do Traefik, **bind explícito**: `"${RADIUS_BIND_ADDR:-127.0.0.1}:1812:1812/udp"` — nunca `0.0.0.0` por padrão |
| **Env** | `${VAR:-default}` com default **seguro** (vazio / `false`). Segredo obrigatório sem default: `${JWT_SECRET}` |
| **Dependência real** | Quando o serviço quebra se o outro não estiver pronto: `healthcheck` no dependido + `condition: service_healthy` |
| **Volumes nomeados** | Dados de banco (`db-data`, `wa-postgres-data`, `gallywix-qdrant-data`) |
| **Bind mounts** | Configuração e conteúdo editável (`nginx.conf:ro`, prompts `:ro`, base de conhecimento `:ro`), e `data/` de runtime dos agentes |
| **Imagens de terceiros** | Tag fixa (`traefik:v2.11`, `qdrant:v1.12.4`, `swagger-ui:v5.17.14`) |
| **Comando customizado** | Tuning vai em `command:` com comentário (ver MySQL abaixo) |

Healthchecks usados:

```yaml
    healthcheck:   # Postgres
      test: ["CMD-SHELL", "pg_isready -U ${WA_POSTGRES_USER} -d ${WA_POSTGRES_DB}"]
      interval: 5s
      timeout: 5s
      retries: 10

    healthcheck:   # Redis
      test: ["CMD", "redis-cli", "ping"]

    healthcheck:   # MySQL (compose de CI)
      test: ['CMD', 'mysqladmin', 'ping', '-h', '127.0.0.1', '-uroot', '-p...']
```

### 5.3 Rede

```yaml
networks:
  default:
    driver: bridge
    enable_ipv6: true
    ipam:
      config:
        - subnet: "fd00:dead:beef:1::/64"
```

Uma rede só. IPv6 ligado porque parte da infra (ex.: VM do Joseph) é alcançada
por IPv6.

### 5.4 Tuning de banco com comentário

```yaml
  db:
    image: mysql:8.0
    mem_limit: 1536m
    # performance_schema pré-aloca centenas de MB que nada consome;
    # buffer pool default (128M) é pouco p/ um dump de 5 GB.
    # (Não comente dentro do bloco `>`: o texto vira argumento do mysqld.)
    command: >
      --performance-schema=OFF
      --innodb-buffer-pool-size=512M
      --innodb-buffer-pool-instances=1
      --max-connections=100
```

> **Admin do banco:** `docker compose exec db mysql ...` — nunca porta exposta.

---

## 6. Roteamento com Traefik

Traefik lê os containers pelo socket do Docker e **só roteia quem tem
`traefik.enable=true`**.

```yaml
  traefik:
    image: traefik:v2.11
    command:
      - "--providers.docker=true"
      - "--providers.docker.exposedbydefault=false"
      - "--entrypoints.web.address=:80"
    ports:
      - "80:80"
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock:ro
```

### 6.1 Um domínio, vários serviços por prefixo

Tudo sob `hexanium.hexanetworks.com.br`, separado por `PathPrefix`, com
**prioridade** explícita (o mais específico ganha; o frontend `/` é o fallback):

| Prefixo | Serviço | Prioridade | Strip do prefixo? |
|---|---|---|---|
| `/api` | backend :4000 | 30 | não (o Express já monta em `/api`) |
| `/xavier`, `/cthullu`, `/joseph` | agentes :8000 | 30 | sim |
| `/wiki` | nginx → wiki.js | 25 | não (nginx trata) |
| `/docs` | swagger-ui :8080 | 20 | sim + redirect `/docs`→`/docs/` |
| `/` | frontend :5173 | 1 | — |

### 6.2 Serviço com prefixo removido (padrão dos agentes)

```yaml
    command: ["uvicorn", "app.api:app", "--host", "0.0.0.0", "--port", "8000",
              "--root-path", "/xavier"]   # OpenAPI gera URLs com o prefixo
    labels:
      - "traefik.enable=true"
      - "traefik.http.routers.xavier.rule=Host(`app.exemplo.com.br`) && PathPrefix(`/xavier`)"
      - "traefik.http.routers.xavier.entrypoints=web"
      - "traefik.http.routers.xavier.priority=30"
      - "traefik.http.routers.xavier.middlewares=xavier-strip"
      - "traefik.http.middlewares.xavier-strip.stripprefix.prefixes=/xavier"
      - "traefik.http.services.xavier.loadbalancer.server.port=8000"
```

- Externamente: `https://app.../xavier/ticket`. Internamente o backend chama
  `http://xavier:8000/ticket` direto, sem passar pelo Traefik.
- `--root-path` faz o "Try it out" do Swagger acertar a URL.
- Em labels, `$` literal vira `$$`.

### 6.3 Serviço movido para outra máquina sem mudar a URL

Quando o Joseph precisou de uma VM com 16 GB (Whisper large), o serviço no
compose virou **um nginx de 64 MB** fazendo proxy para o IP da VM. PABX e
Discord continuaram chamando a mesma URL pública. Padrão bom para escalar um
componente pesado sem migrar clientes.

> **⚠ No projeto novo:**
> - A origem aceita qualquer um na :80 — hoje dá para contornar a Cloudflare
>   batendo direto no IP (`docs/swagger-docs-security.md`, F1). Restrinja a :80/:443
>   às faixas da Cloudflare no firewall do host **ou** use Cloudflare Tunnel.
> - Use **Traefik v3** e TLS de origem (certificado de origem da Cloudflare, modo
>   *Full (strict)*) em vez de HTTP puro entre Cloudflare e servidor.
> - Declare o `Host(...)` via variável para não repetir o domínio em cada label.

---

## 7. Configuração e segredos

### 7.1 Três lugares, papéis diferentes

| Arquivo | Versionado | Conteúdo |
|---|---|---|
| `docker-compose.yml` | sim | **quais** variáveis cada serviço recebe + defaults seguros |
| `.env.example` | sim | todas as chaves, **sem valor**, com comentário de onde tirar e como gerar |
| `.env` | **nunca** | valores reais; `chmod 600`; cópia `.env.bak-<motivo>` antes de mexer (também gitignored) |

### 7.2 Convenções

- Prefixo por serviço: `XAVIER_*`, `RYUK_*`, `GALLYWIX_*`, `WA_*`, `VPN_*`.
- Feature flags / kill-switches: `*_ENABLED`, `*_ACTIONS_ENABLED`,
  `*_SEND_ENABLED`, `*_DISPATCH_ENABLED` — **default `false`** quando a ação
  atinge cliente ou sistema externo. Exemplo real:
  `RYUK_FOLLOWUP_SEND_ENABLED: ${RYUK_FOLLOWUP_SEND_ENABLED:-false}  # NADA sai ao cliente com false`.
- Serviço-para-serviço autenticado com **token compartilhado** em env
  (`WHATSAPP_INTERNAL_WEBHOOK_TOKEN`, `X-API-Key` dos agentes). O agente é
  **fail-closed**: sem chave configurada, recusa.
- Chaves de criptografia de dados **separadas** do `JWT_SECRET`
  (`ATTACHMENTS_ENCRYPTION_KEY`, `FINANCE_ENCRYPTION_KEY`). Credenciais de
  integrações salvas pela UI ficam cifradas no banco.
- O backend faz **checagem de configuração no boot** (`configSafety.js`) e loga
  `[security]` quando encontra segredo fraco/default.
- Rotação documentada em runbook (`docs/secret-rotation.md`): backup →
  auditar → re-cifrar com a chave nova → só então trocar o segredo antigo.

> **⚠ No projeto novo:**
> - `.env` e `.gitignore` **no primeiro commit**. O Hexanium teve `.env` com
>   segredo real no histórico e precisou de rotação.
> - Não coloque defaults do tipo `change-me-...` para tokens; prefira vazio +
>   recusar subir. Senhas fixas no compose (como a do wiki-db) também não.
> - `gitleaks` no CI desde o dia 1.

---

## 8. Banco de dados e migrations

- **Schema inicial**: `db/init.sql` montado em `/docker-entrypoint-initdb.d/`
  (roda só quando o volume está vazio).
- **Evolução**: o backend, no boot (`startup/database.js`), executa
  `CREATE TABLE IF NOT EXISTS` / ajustes idempotentes antes de abrir a porta.
  Se falhar, `process.exit(1)` e o Docker reinicia.
- **Seed**: usuário admin criado a partir de `AUTH_USER`/`AUTH_PASSWORD`;
  permissões RBAC sincronizadas com o código a cada boot.
- **Bancos por domínio**: MySQL para o ERP; Postgres dedicado para a Evolution;
  Postgres dedicado para a Wiki; Qdrant para vetores. Cada terceiro tem o **seu**
  banco — ninguém compartilha schema.
- **Usuário dedicado com privilégio mínimo** para consumidores externos
  (FreeRADIUS só tem `SELECT` nas views que precisa).

> **⚠ No projeto novo:** migrations versionadas (tabela `schema_migrations` +
> arquivos numerados, ou uma ferramenta como Knex/Prisma/Alembic) em vez de
> DDL idempotente espalhado. Escala melhor e deixa claro o que rodou.

---

## 9. Agentes de IA — padrão de serviço

Todos seguem o mesmo contrato, o que facilita criar um novo copiando outro:

1. **FastAPI** na porta 8000, `GET /health`, OpenAPI com `--root-path`.
2. **Interno por padrão.** Só ganha label do Traefik se recebe webhook de fora
   (OTRS, n8n, PABX), e aí com `X-API-Key`.
3. **LLM trocável** por `<AGENTE>_LLM_BACKEND` (`ollama_cloud` | `claude` | local),
   com modelo e timeout por env.
4. **Prompt de sistema em arquivo `.md` montado `:ro`** → ajustar o prompt é
   editar e `docker compose restart`, sem rebuild.
5. **Base de conhecimento montada, não assada** → reindexar sem rebuild.
6. **`data/` em bind mount** (cache de modelos com `HF_HOME=/app/data/hf`,
   saídas, estado). Gitignored.
7. **Modo shadow primeiro**: o agente sugere/loga/manda no Discord; a ação real
   (`*_ACTIONS_ENABLED`) só liga depois de validado.
8. **Modelos pesados carregados sob demanda e descarregados por ociosidade**
   (`*_IDLE_TTL_S`) — Whisper e fastembed não ficam residentes à toa.
9. **Notificação operacional** em webhook do Discord.

---

## 10. CI (GitHub Actions)

| Workflow | Gatilho | O que faz |
|---|---|---|
| `test.yml` | PR + push na `main` | `npm install` + `npm test` no backend e no frontend (node `--test`), com cache do npm |
| `e2e.yml` | PR + push na `main` | sobe `docker-compose.ci.yml`, espera app responder, roda Playwright (Chromium) e despeja logs se falhar |
| `secret-scan.yml` | PR + push na `main` | gitleaks com `fetch-depth: 0` |

**Compose de CI** (`docker-compose.ci.yml`): só `db + backend + frontend`, sem
Traefik (o rewrite do Next resolve `/api`), segredos descartáveis
(`ci-e2e-jwt-secret-not-for-production-32`), schema + seed carregados por
`docker-entrypoint-initdb.d`, `healthcheck` no MySQL e `service_healthy` no
backend. É o mesmo Dockerfile de produção.

Espera de prontidão no E2E: loop de `curl` verificando frontend `200` e API
diferente de `000`/`502`, até 5 minutos.

---

## 11. Versionamento, release e deploy

### 11.1 Versão

- **SemVer** único para backend e frontend (`0.3.389`), bumpado em todo PR
  mergeado.
- `scripts/bump-version.sh [--patch | --version X.Y.Z] [--dry-run]` atualiza
  `package.json`, `package-lock.json`, `config.js` e os defaults do compose de
  uma vez; `scripts/check-version-consistency.sh` valida que não ficou nada
  para trás.
- A versão chega ao frontend como build-arg (`NEXT_PUBLIC_APP_VERSION`) e ao
  backend como env (`BACKEND_APP_VERSION`) — a UI mostra o que está rodando.

### 11.2 Release

`scripts/publish-release.sh --version X.Y.Z --notes-file ...`: roda testes,
build do frontend, cria tag `vX.Y.Z` e GitHub Release, e opcionalmente registra
a versão no próprio sistema (`/api/system/releases`).

### 11.3 Deploy

O deploy é **no próprio servidor**, com build local:

```bash
cd /opt/hexanium
git pull
docker compose up -d --build backend frontend   # só o que mudou
docker compose logs -f backend
```

Também existe **deploy pela UI** (Configurações › Atualizações):

```
UI ─► backend (/api/system/update, lock no MySQL)
        └─► POST http://update-webhook:8099/deploy  (Bearer token)
              └─► docker compose -f /workspace/docker-compose.yml up -d --build backend frontend
        └─► healthcheck GET /api/health (12 tentativas × 5 s)
```

O `update-webhook` é um container Node mínimo com `docker-cli` +
`docker-cli-compose`, o socket do Docker e o repositório montados. Ele também
oferece `POST /restart` com **whitelist** (`evolution-api,wa-gateway`) e
anti-storm (1 reinício por minuto por serviço), usado pelo watchdog do backend
quando a ingestão do WhatsApp trava.

> **⚠ No projeto novo:** montar `/var/run/docker.sock` em container é dar root
> no host a ele. Se for manter o deploy por botão, mantenha o container sem porta
> publicada, token forte e whitelist de comandos. Alternativa mais limpa: CI
> builda e publica imagem no GHCR, e o servidor faz `docker compose pull && up -d`.

---

## 12. Operação

### 12.1 Memória (o servidor é compartilhado por ~20 containers)

- `mem_limit` por serviço, **medido** (`docker stats`) e com folga para o pico
  conhecido.
- `scripts/mem/baseline.sh` — tira foto do consumo (antes/depois de mudanças).
- `scripts/mem/mem-watch.sh` — cron de hora em hora; alerta no Discord quando
  um container passa de 80% do próprio teto, swap do host passa de 50% ou houve
  OOM-kill.
  ```cron
  0 * * * * sg docker -c '/opt/hexanium/scripts/mem/mem-watch.sh' >> /tmp/mem-watch.log 2>&1
  ```
- `scripts/mem/host-tuning.sh` (root) — `vm.swappiness=10` e limite do journald.

### 12.2 Backups

- Dumps em `db/backups/` (gitignored), nomeados `<banco>_<AAAAMMDD_HHMMSS>.sql.gz`.
  ```bash
  docker compose exec -T db mysqldump -uroot -p"$MYSQL_ROOT_PASSWORD" \
    --single-transaction hexanium_erp | gzip > db/backups/hexanium_erp_$(date +%Y%m%d_%H%M%S).sql.gz
  ```
- `scripts/mem/rotate-backups.sh` — comprime e mantém as `KEEP=3` mais recentes
  por prefixo; **simulação por padrão**, só apaga com `--apply`.

> **⚠ No projeto novo:** backup agendado em cron **e copiado para fora do
> servidor** (S3/B2/outro host), com teste de restore periódico. Hoje os dumps
> ficam no mesmo disco.

### 12.3 Processo

- Backend registra `unhandledRejection` sem derrubar e sai em
  `uncaughtException` — o `restart: unless-stopped` traz de volta.
- Logs: stdout/stderr dos containers (`docker compose logs`).
- Scripts operacionais seguem o padrão: `set -euo pipefail`, `--help`,
  `--dry-run`/simulação por padrão, e ação destrutiva só com flag explícita
  (`--apply`, `--commit --i-have-a-backup`, `CONFIRM=yes`).

---

## 13. Checklist para iniciar o projeto novo

**Dia 1**
- [ ] Monorepo com `backend/`, `frontend/`, `agents/`, `infra/`, `scripts/`, `docs/`.
- [ ] `.gitignore` com `.env`, `*.bak-*`, `node_modules/`, `.next/`, `dist/`, `agents/*/data/`, `db/backups/`.
- [ ] `.env.example` comentado; `.env` com `chmod 600`.
- [ ] `docker-compose.yml` com Traefik (`exposedbydefault=false`), rede única, `restart: unless-stopped`.
- [ ] Banco sem porta publicada; admin via `docker compose exec`.
- [ ] Dockerfiles multi-stage, `npm ci`, `USER` não-root, `.dockerignore`.
- [ ] `GET /api/health` no backend e `GET /health` em cada serviço.
- [ ] Next.js com `rewrites` de `/api` e headers de segurança.
- [ ] `docker-compose.ci.yml` mínimo + workflows `test`, `e2e`, `secret-scan`.
- [ ] `scripts/bump-version.sh` e versão exibida na UI.

**Antes de ir para produção**
- [ ] Origem só aceita tráfego da Cloudflare (firewall ou Tunnel); TLS Full (strict).
- [ ] Nenhum default `change-me` em segredo; boot recusa/avisa segredo fraco.
- [ ] `mem_limit` medido em cada serviço + `mem-watch` no cron.
- [ ] Backup agendado, **fora do servidor**, com restore testado.
- [ ] Toda integração que age externamente com kill-switch `false` por padrão.
- [ ] Migrations versionadas.
- [ ] Runbook de rotação de segredos em `docs/`.
