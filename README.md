# Slack CLI

[![CI](https://github.com/junyeong-ai/slack-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/slack-cli/actions)
[![Rust](https://img.shields.io/badge/rust-1.95.0%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
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
slack-cli send "#general" "Hello!"
```

---

## 주요 기능

### 메시지
```bash
slack-cli send "#general" "공지사항입니다"      # 전송
slack-cli update "#general" 1234.5678 "수정됨"  # 수정
slack-cli delete "#general" 1234.5678           # 삭제
slack-cli messages "#general" --limit 15        # 조회
slack-cli messages "#general" --oldest 2025-01-01 --latest 2025-01-31  # 날짜 필터
slack-cli messages "#general" --exclude-bots    # 봇 메시지 제외
slack-cli messages "#general" --expand date,user_name  # 날짜/이름 확장
slack-cli thread "#general" 1234.5678           # 스레드
slack-cli search "키워드" --sort timestamp     # Real-time Search
```

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

`install.sh`는 GitHub Release의 사전 빌드 바이너리와 SHA-256 체크섬을 내려받아 검증한 뒤 `~/.local/bin/slack-cli`에 설치합니다. 같은 실행 안에서 Claude Code 스킬도 `~/.claude/skills/slack-workspace`에 설치할 수 있으므로 저장소를 clone할 필요가 없습니다.

```bash
# 특정 릴리스 설치
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/install.sh | SLACK_CLI_VERSION=v0.4.0 bash

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
cargo +1.95.0 build --release
```

**요구사항**: Rust 1.95.0+

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
users:read  users:read.email  chat:write
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
users_fields = ["id", "name", "real_name", "email"]
channels_fields = ["id", "name", "type", "members"]

[connection]
api_base_url = "https://slack.com/api"
rate_limit_per_minute = 20
app_distribution = "commercial_external"
timeout_seconds = 30
```

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
| `send <ch> <text>` | 메시지 전송 |
| `update <ch> <ts> <text>` | 메시지 수정 |
| `delete <ch> <ts>` | 메시지 삭제 |
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
- `--expand <fields>` — 추가 필드
  - users: `avatar`, `title`, `timezone`, `status`, `is_admin`, `is_bot`, `deleted`
  - channels: `topic`, `purpose`, `created`, `creator`, `is_archived`, `is_private`

### send 옵션
- `--thread <ts>` — 스레드 답장

### messages/thread 옵션
- `--limit <N>` — 결과 제한 (기본: `15`)
- `--oldest <date>` — 시작 시간 (Unix timestamp 또는 YYYY-MM-DD)
- `--latest <date>` — 종료 시간 (Unix timestamp 또는 YYYY-MM-DD)
- `--exclude-bots` — 봇 메시지 제외
- `--expand <fields>` — 추가 필드: `date`, `user_name`

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
