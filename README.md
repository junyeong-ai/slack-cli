# Slack CLI

[![CI](https://github.com/junyeong-ai/slack-cli/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/junyeong-ai/slack-cli/actions/workflows/ci.yml?query=branch%3Amain)
[![Rust](https://img.shields.io/badge/rust-1.97.0%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![DeepWiki](https://img.shields.io/badge/DeepWiki-junyeong--ai%2Fslack--cli-blue.svg?logo=data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACwAAAAyCAYAAAAnWDnqAAAAAXNSR0IArs4c6QAAA05JREFUaEPtmUtyEzEQhtWTQyQLHNak2AB7ZnyXZMEjXMGeK/AIi+QuHrMnbChYY7MIh8g01fJoopFb0uhhEqqcbWTp06/uv1saEDv4O3n3dV60RfP947Mm9/SQc0ICFQgzfc4CYZoTPAswgSJCCUJUnAAoRHOAUOcATwbmVLWdGoH//PB8mnKqScAhsD0kYP3j/Yt5LPQe2KvcXmGvRHcDnpxfL2zOYJ1mFwrryWTz0advv1Ut4CJgf5uhDuDj5eUcAUoahrdY/56ebRWeraTjMt/00Sh3UDtjgHtQNHwcRGOC98BJEAEymycmYcWwOprTgcB6VZ5JK5TAJ+fXGLBm3FDAmn6oPPjR4rKCAoJCal2eAiQp2x0vxTPB3ALO2CRkwmDy5WohzBDwSEFKRwPbknEggCPB/imwrycgxX2NzoMCHhPkDwqYMr9tRcP5qNrMZHkVnOjRMWwLCcr8ohBVb1OMjxLwGCvjTikrsBOiA6fNyCrm8V1rP93iVPpwaE+gO0SsWmPiXB+jikdf6SizrT5qKasx5j8ABbHpFTx+vFXp9EnYQmLx02h1QTTrl6eDqxLnGjporxl3NL3agEvXdT0WmEost648sQOYAeJS9Q7bfUVoMGnjo4AZdUMQku50McDcMWcBPvr0SzbTAFDfvJqwLzgxwATnCgnp4wDl6Aa+Ax283gghmj+vj7feE2KBBRMW3FzOpLOADl0Isb5587h/U4gGvkt5v60Z1VLG8BhYjbzRwyQZemwAd6cCR5/XFWLYZRIMpX39AR0tjaGGiGzLVyhse5C9RKC6ai42ppWPKiBagOvaYk8lO7DajerabOZP46Lby5wKjw1HCRx7p9sVMOWGzb/vA1hwiWc6jm3MvQDTogQkiqIhJV0nBQBTU+3okKCFDy9WwferkHjtxib7t3xIUQtHxnIwtx4mpg26/HfwVNVDb4oI9RHmx5WGelRVlrtiw43zboCLaxv46AZeB3IlTkwouebTr1y2NjSpHz68WNFjHvupy3q8TFn3Hos2IAk4Ju5dCo8B3wP7VPr/FGaKiG+T+v+TQqIrOqMTL1VdWV1DdmcbO8KXBz6esmYWYKPwDL5b5FA1a0hwapHiom0r/cKaoqr+27/XcrS5UwSMbQAAAABJRU5ErkJggg==)](https://deepwiki.com/junyeong-ai/slack-cli)

> **[English](README.en.md)** | **한국어**

**터미널에서 Slack 주요 워크플로를 빠르게 처리하세요.** 메시지 전송, 검색, 리액션, 핀, 북마크, 사용자/채널 조회를 브라우저 없이 수행할 수 있습니다.

---

## 왜 Slack CLI인가?

- **빠름** — SQLite FTS5 기반 밀리초 단위 검색
- **실용적** — 메시지, 검색, 리액션, 핀, 북마크, 사용자/채널 조회 지원
- **자동화** — 스크립트, CI/CD, AI 에이전트와 통합 가능

---

## 빠른 시작

```bash
# 설치
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/install.sh | bash

# 로그인 (브라우저 OAuth)
slack-cli auth login --client-id <your-client-id>

# 또는 기존 토큰을 붙여넣기
slack-cli auth login --user-token xoxp-your-token

# 사용
slack-cli cache refresh
slack-cli users "john"
slack-cli send "#general" -t "Hello!"
```

---

## 주요 기능

### 채널 식별자 (`<channel>` 인자)

`#name` · `name` (캐시 lookup) · `C…/G…` 채널 ID · `D…` DM ID · `U…/W…` 유저 ID (해당 유저와의 DM 채널로 자동 해석 — `cache refresh` 시점에 `channel_types` 에 `im` 포함 필요)

### 메시지
```bash
slack-cli send "#general" -t "공지사항입니다"           # 전송 (텍스트)
slack-cli send "#general" --markdown-text "**볼드**"    # 전송 (표준 Markdown, Slack이 렌더링)
slack-cli send U123ABCDEF -t "DM by user-id"           # 유저 ID → DM 자동 해석
slack-cli send "#general" -b @blocks.json -t "fallback" # Block Kit + 폴백 텍스트
slack-cli send "#general" -m @meta.json -t "deploy done" # 멱등 metadata 첨부
echo '{"event_type":"x","event_payload":{}}' | slack-cli send "#general" -t "x" -m -
slack-cli update "#general" 1234.5678 -t "수정됨"       # 수정 (text/markdown_text/blocks/attachments/metadata)
slack-cli delete "#general" 1234.5678                   # 삭제
slack-cli permalink "#general" 1234.5678                # permalink URL 조회
slack-cli messages "#general" --limit 15                # 조회 (lean 기본 필드)
slack-cli messages "#general" --expand blocks,reactions # 필드 확장
slack-cli messages "#general" --oldest 2025-01-01 --latest 2025-01-31
slack-cli messages "#general" --exclude-bots            # 봇 메시지 제외
slack-cli messages "#general" --cursor <next_cursor>    # 다음 페이지 (JSON 출력의 next_cursor)
slack-cli thread "#general" 1234.5678                   # 스레드
slack-cli search "키워드" --sort timestamp              # Real-time Search
```

**JSON 입력**: `--blocks` / `--attachments` / `--metadata`는 세 가지 입력 형태를 지원합니다.

| 형식 | 의미 |
|---|---|
| `-` | stdin 에서 읽기 (한 호출에 최대 1회) |
| `@path.json` | 파일에서 읽기 |
| 그 외 | inline JSON 리터럴 |

### 리액션
```bash
slack-cli react "#general" 1234.5678 thumbsup   # 추가
slack-cli unreact "#general" 1234.5678 thumbsup # 제거
slack-cli reactions "#general" 1234.5678        # 조회
```

### 핀 & 북마크
```bash
slack-cli pin "#general" 1234.5678              # 핀 추가
slack-cli unpin "#general" 1234.5678            # 핀 제거
slack-cli pins "#general"                       # 핀 목록

slack-cli bookmark "#general" "Wiki" "https://..."  # 북마크 추가
slack-cli bookmarks "#general"                      # 북마크 목록
```

### 검색 & 조회
```bash
slack-cli users "john" --limit 10               # 사용자 검색
slack-cli users --id U123,U456                  # ID로 조회
slack-cli users "john" --expand avatar,title    # 추가 필드 포함
slack-cli channels "dev"                        # 채널 검색
slack-cli channels --id C123,C456               # ID로 조회
slack-cli channels "dev" --expand topic,purpose # 추가 필드 포함
slack-cli members "#dev-team"                   # 멤버 목록
slack-cli emoji --query "party"                 # 이모지 검색
```

### 인증 & 캐시 & 설정
```bash
slack-cli auth login                            # 새 워크스페이스 로그인 (기본: PKCE)
slack-cli auth login --method static --user-token xoxp-...  # 토큰 붙여넣기
slack-cli auth profiles                         # 저장된 프로필 목록
slack-cli auth status --verify                  # 활성 프로필 검증
slack-cli auth use work                         # 활성 프로필 전환
slack-cli auth logout                           # 활성 프로필 제거

slack-cli --profile work users "john"           # 특정 프로필로 1회 호출

slack-cli cache stats                           # 캐시 상태
slack-cli cache refresh                         # 캐시 새로고침
slack-cli config show                           # 설정 표시
```

---

## 설치

### 자동 설치 (권장)
```bash
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/install.sh | bash
```

`install.sh`는 GitHub Release의 사전 빌드 바이너리를 내려받아 SHA-256 체크섬을 검증하고, `cosign`이 설치돼 있으면 sigstore 서명까지 검증한 뒤 `~/.local/bin/slack-cli`에 설치합니다. Linux에서는 glibc/musl을 자동 감지합니다. 같은 실행 안에서 Claude Code 스킬도 `~/.claude/skills/slack-workspace`에 설치할 수 있으므로 저장소를 clone할 필요가 없습니다.

```bash
# 특정 릴리스 설치
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/install.sh | SLACK_CLI_VERSION=v0.5.0 bash

# 제거 (비대화형 기본값은 바이너리만 제거하고 스킬/설정은 보존)
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/uninstall.sh | bash

# 스킬과 설정까지 제거
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/uninstall.sh | bash -s -- --yes
```

### Cargo (Git)
```bash
cargo install --locked --git https://github.com/junyeong-ai/slack-cli
```

### 소스 빌드
```bash
git clone https://github.com/junyeong-ai/slack-cli && cd slack-cli
cargo build --release   # rust-toolchain.toml이 1.97.0 툴체인을 자동 선택
```

**요구사항**: Rust 1.97.0+ (rustup)

---

## 인증

`slack-cli`는 토큰을 `~/.config/slack-cli/auth.json`에 0600 권한으로 저장하며, 워크스페이스마다 명명된 프로필로 관리합니다. `config.toml`에는 토큰이 들어가지 않습니다.

### 방법 1 — PKCE OAuth (브라우저 흐름, 권장)

```bash
slack-cli auth login --client-id <client-id>
# 또는 환경변수
SLACK_CLI_CLIENT_ID=<client-id> slack-cli auth login
```

`auth login`이 로컬 콜백 서버를 `127.0.0.1:53682`에 잠깐 띄우고, 브라우저로 Slack 인증 페이지를 열어 코드를 받아 user token을 발급받습니다. 사전 준비:

1. [api.slack.com/apps](https://api.slack.com/apps)에서 앱 생성
2. **OAuth & Permissions** → User Token Scopes에 아래 항목 추가
3. **Redirect URLs**에 `http://127.0.0.1:53682/callback` 등록
4. **Manage Distribution**에서 PKCE 옵션 활성화 후 client_id 복사

**User Token Scopes** (전체 기능 사용 시):
```
channels:read  channels:history  groups:read  groups:history
im:read  im:history  mpim:read  mpim:history
users:read  users:read.email  chat:write  metadata.message:read
reactions:read  reactions:write  pins:read  pins:write
bookmarks:read  bookmarks:write  emoji:read  search:read
```

### 방법 2 — 토큰 직접 붙여넣기 (Static)

기존 발급된 `xoxp-` / `xoxb-` 토큰이 있을 때:

```bash
slack-cli auth login --method static --user-token xoxp-your-token
# 봇 토큰을 함께 등록
slack-cli auth login --method static --user-token xoxp-... --bot-token xoxb-...
```

`auth.test`로 토큰을 검증한 뒤 프로필이 저장됩니다.

### 프로필 관리

```bash
slack-cli auth profiles                  # 목록
slack-cli auth status --verify           # 활성 프로필 + auth.test 검증
slack-cli auth use work                  # 활성 프로필 전환
slack-cli --profile work users "john"    # 1회 호출에만 다른 프로필 사용
slack-cli auth logout                    # 활성 프로필 제거
slack-cli auth logout --all              # 모든 프로필 제거
```

`--profile NAME`은 글로벌 플래그로 어느 위치에도 둘 수 있습니다.

---

## 설정 파일

`~/.config/slack-cli/config.toml` (사용자 환경설정, 토큰 없음):

```toml
[cache]
ttl_users_hours = 168          # 1주일
ttl_channels_hours = 168
refresh_threshold_percent = 10 # TTL의 10% 시점부터 stale 경고
channel_types = ["public_channel", "private_channel"]
                               # 캐시할 conversation 타입.
                               # 토큰 scope에 맞춰 조정 (public만 있으면 ["public_channel"]).
                               # 허용 값: public_channel, private_channel, mpim, im

[output]
users_fields    = ["id", "name", "real_name", "email"]
channels_fields = ["id", "name", "type", "members"]
messages_fields = ["ts", "user", "bot_id", "username", "text", "thread_ts", "reply_count", "subtype", "metadata"]

[connection]
api_base_url = "https://slack.com/api"
rate_limit_per_minute = 20
app_distribution = "commercial_external"
timeout_seconds = 30
```

알 수 없는 키는 무시되지 않고 오류로 처리됩니다 — 이전 버전의 잔여 키(`user_token`, `bot_token`, `max_idle_per_host`, `pool_idle_timeout_seconds`)가 있으면 명시적 에러로 표면화되니 제거하세요.

`app_distribution`은 Slack의 `conversations.history`/`conversations.replies` 제한 정책에 맞춥니다. Slack Marketplace 승인 앱 또는 내부 고객 제작 앱이면 `marketplace_or_internal`로 설정할 수 있습니다.

### 환경변수

| 변수 | 용도 |
|---|---|
| `SLACK_USER_TOKEN` | 저장된 프로필을 무시하고 이 토큰을 직접 사용 (CI/headless) |
| `SLACK_BOT_TOKEN` | 위와 동일, bot 토큰 |
| `SLACK_PROFILE` | 활성 프로필 1회 override (= 글로벌 `--profile`) |
| `SLACK_CLI_CLIENT_ID` | PKCE 로그인 시 client_id (= `--client-id`) |

---

## 명령어 참조

| 명령어 | 설명 |
|--------|------|
| `auth login` | 워크스페이스 인증 (`--method pkce\|static`) |
| `auth logout [--all]` | 프로필 제거 (`--keep-remote`로 `auth.revoke` 생략) |
| `auth status [--verify]` | 프로필 상태 + 선택적 토큰 검증 |
| `auth profiles` | 저장된 프로필 목록 |
| `auth use <name>` | 활성 프로필 전환 |
| `users <query>` | 사용자 검색 |
| `users --id <ids>` | ID로 조회 (쉼표 구분) |
| `channels <query>` | 채널 검색 |
| `channels --id <ids>` | ID로 조회 (쉼표 구분) |
| `send <ch> [-t -b -a -m --markdown-text --thread]` | 메시지 전송 (content 필드 ≥1 필수) |
| `update <ch> <ts> [-t -b -a -m --markdown-text]` | 메시지 수정 (content 필드 ≥1 필수) |
| `delete <ch> <ts>` | 메시지 삭제 |
| `permalink <ch> <ts>` | 메시지 permalink URL 조회 |
| `messages <ch>` | 메시지 조회 |
| `thread <ch> <ts>` | 스레드 조회 |
| `members <ch>` | 멤버 목록 |
| `search <query>` | Real-time Search API 검색 |
| `react <ch> <ts> <emoji>` | 리액션 추가 |
| `unreact <ch> <ts> <emoji>` | 리액션 제거 |
| `reactions <ch> <ts>` | 리액션 조회 |
| `emoji` | 이모지 목록 |
| `pin <ch> <ts>` | 핀 추가 |
| `unpin <ch> <ts>` | 핀 제거 |
| `pins <ch>` | 핀 목록 |
| `bookmark <ch> <title> <url>` | 북마크 추가 |
| `unbookmark <ch> <id>` | 북마크 제거 |
| `bookmarks <ch>` | 북마크 목록 |
| `cache stats/refresh` | 캐시 관리 |
| `config show/path/edit` | 설정 관리 |

### 공통 옵션
- `--json` — JSON 출력
- `--profile <name>` — 1회 호출에 사용할 프로필 (env: `SLACK_PROFILE`)
- `--config <path>` — config.toml 경로 override
- `--verbose` — debug 로그 활성

### users/channels 옵션
- `--limit <N>` — 결과 제한 (기본: `10`)
- `--id <ids>` — ID로 조회 (쉼표 구분)
- `--expand <fields>` — 기본 필드 외 추가로 노출할 필드
  - users: `display_name`, `status`, `status_emoji`, `avatar`, `title`, `timezone`, `is_admin`, `is_bot`, `deleted`
  - channels: `topic`, `purpose`, `created`, `creator`, `is_member`, `is_archived`, `is_private`, `user` (DM 의 상대 user id)

### send / update 옵션
- `-t, --text <TEXT>` — 메시지 텍스트 (blocks 동반 시 알림 폴백)
- `--markdown-text <TEXT>` — 표준 Markdown 본문 (Slack이 렌더링, 최대 12,000자). `--text`/`--blocks` 와 동시 사용 불가
- `-b, --blocks <SOURCE>` — Block Kit blocks (JSON array). `-` / `@file` / inline
- `-a, --attachments <SOURCE>` — Legacy attachments (JSON array). 동일 SOURCE 어휘
- `-m, --metadata <SOURCE>` — Message metadata `{event_type, event_payload}` (JSON object). 동일 SOURCE 어휘
- `--thread <ts>` — (send 전용) 스레드 답장

`text`/`markdown_text`/`blocks`/`attachments` 중 최소 하나는 반드시 제공해야 합니다. 같은 호출에서 `-` (stdin) 은 최대 한 플래그에만 사용 가능합니다.

### messages/thread 옵션
- `--limit <N>` — 결과 제한 (기본: `15`)
- `--cursor <cursor>` — (messages 전용) 이전 응답의 `next_cursor` 로 다음 페이지 조회
- `--oldest <date>` — (messages 전용) 시작 시간 (Unix timestamp 또는 YYYY-MM-DD)
- `--latest <date>` — (messages 전용) 종료 시간 (Unix timestamp 또는 YYYY-MM-DD)
- `--exclude-bots` — 봇 메시지 제외 (messages·thread 공통)
- `--expand <fields>` — 기본 필드에 추가로 노출할 필드
  - 계산 필드: `date`, `user_name`
  - 응답 필드: `blocks`, `attachments`, `reactions`, `edited`, `parent_user_id`, `reply_users`, `reply_users_count`, `latest_reply`, `channel`, `permalink`

`messages_fields` 기본값(lean): `ts`, `user`, `bot_id`, `username`, `text`, `thread_ts`, `reply_count`, `subtype`, `metadata`. AI 에이전트 컨텍스트 절약을 위해 기본 출력은 가볍게 유지하며, 풍부한 필드는 `--expand` 로 명시 opt-in 합니다.

`messages --json` 출력은 `{messages: [...], next_cursor}` 봉투입니다. `next_cursor` 가 `null` 이 아니면 같은 명령에 `--cursor` 로 넘겨 다음 페이지를 조회합니다. `thread --json` 은 `--limit` 까지 내부 페이징하므로 배열 그대로입니다.

### 종료 코드 & 오류 출력

| 코드 | 의미 |
|---|---|
| `0` | 성공 |
| `1` | 일반 오류 |
| `2` | 사용법 오류 (clap) |
| `3` | 인증 오류 (재로그인 필요 — `invalid_auth`, `missing_scope` 등) |
| `4` | 레이트리밋 (재시도 소진) |

`--json` 모드의 런타임 실패는 stderr 로 `{"error": {"code", "message"}}` 봉투를 출력합니다. 사용법 오류(종료 코드 `2`)는 파싱 단계에서 발생하므로 clap 의 진단 텍스트가 그대로 출력됩니다 — 종료 코드만으로 구분하면 됩니다. `code` 는 Slack API 오류면 Slack 의 오류 문자열 그대로(`channel_not_found` 등), 그 외에는 `auth_error` / `rate_limited` / `http_error` / `network_error` / `error` 입니다. stdout 은 항상 "파싱 가능한 데이터 또는 빈 값"을 유지합니다.

### search 옵션
- `--limit <N>` — 총 결과 수 (1-100, 기본: `10`. 20개 단위 페이지로 자동 페이징)
- `--channel <id|name>` — 특정 채널로 검색 한정
- `--before <date>` — 이 시점 이전 결과만 (Unix ts 또는 YYYY-MM-DD)
- `--after <date>` — 이 시점 이후 결과만
- `--channel-types <types>` — 검색할 대화 타입 (기본: `public_channel,private_channel,mpim,im`)
- `--content-types <types>` — 검색 대상 (기본: `messages`)
- `--include-context` — 검색 결과 주변 맥락 포함
- `--include-bots` — 봇 메시지 포함
- `--include-archived` — 아카이브 채널 포함
- `--no-semantic` — 키워드 일치만 사용 (시맨틱 검색 비활성)
- `--sort <score|timestamp>` — 정렬 기준
- `--sort-dir <asc|desc>` — 정렬 방향

---

## 문제 해결

### 캐시 초기화
```bash
rm -rf ~/.config/slack-cli/cache && slack-cli cache refresh
```

### 권한 오류
토큰 scope 확인 → Workspace 재설치 → 새 scope 반영 위해 `slack-cli auth login` 재실행

### 디버그
```bash
RUST_LOG=debug slack-cli users "john"
```

---

## 지원

- [GitHub Issues](https://github.com/junyeong-ai/slack-cli/issues)
- [개발자 가이드](CLAUDE.md)

---

<div align="center">

**[English](README.en.md)** | **한국어**

Made with Rust

</div>
