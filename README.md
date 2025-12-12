# Slack CLI

[![CI](https://github.com/junyeong-ai/slack-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/slack-cli/actions)
[![Rust](https://img.shields.io/badge/rust-1.91.1%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)

> **[English](README.en.md)** | **한국어**

**터미널에서 Slack을 완전히 제어하세요.** 메시지 전송부터 리액션, 핀, 북마크까지 — 브라우저 없이 모든 작업을 수행할 수 있습니다.

---

## 왜 Slack CLI인가?

- **빠름** — SQLite FTS5 기반 밀리초 단위 검색
- **완전함** — 21개 명령어로 Slack 전체 기능 커버
- **자동화** — 스크립트, CI/CD, AI 에이전트와 통합 가능

---

## 빠른 시작

```bash
# 설치
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/install.sh | bash

# 설정
slack-cli config init --bot-token xoxb-your-token
slack-cli cache refresh

# 사용
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
slack-cli messages "#general" --limit 20        # 조회
slack-cli messages "#general" --oldest 2025-01-01 --latest 2025-01-31  # 날짜 필터
slack-cli messages "#general" --exclude-bots    # 봇 메시지 제외
slack-cli messages "#general" --expand date,user_name  # 날짜/이름 확장
slack-cli thread "#general" 1234.5678           # 스레드
slack-cli search "키워드" --channel "#dev"      # 검색
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

### 캐시 & 설정
```bash
slack-cli cache stats                           # 상태 확인
slack-cli cache refresh                         # 새로고침
slack-cli config show                           # 설정 표시
```

---

## 설치

### 자동 설치 (권장)
```bash
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/install.sh | bash
```

### Cargo
```bash
cargo install slack-cli
```

### 소스 빌드
```bash
git clone https://github.com/junyeong-ai/slack-cli && cd slack-cli
cargo build --release
```

**요구사항**: Rust 1.91.1+

---

## Slack 토큰 설정

### 1. 앱 생성
[api.slack.com/apps](https://api.slack.com/apps) → Create New App → From scratch

### 2. 권한 추가

**User Token Scopes** (권장):
```
channels:read  channels:history  groups:read  groups:history
im:read  im:history  mpim:read  mpim:history
users:read  users:read.email  chat:write  search:read
reactions:read  reactions:write  pins:read  pins:write
bookmarks:read  bookmarks:write  emoji:read
```

### 3. 설치 및 토큰 복사
Install to Workspace → `xoxp-...` 토큰 복사

### 4. CLI 설정
```bash
slack-cli config init --user-token xoxp-your-token
```

---

## 설정

### 환경 변수
```bash
export SLACK_USER_TOKEN="xoxp-..."
export SLACK_BOT_TOKEN="xoxb-..."
```

### 설정 파일
`~/.config/slack-cli/config.toml`:
```toml
user_token = "xoxp-..."
bot_token = "xoxb-..."

[cache]
ttl_users_hours = 168          # 1주일
ttl_channels_hours = 168
refresh_threshold_percent = 10 # TTL의 10% 시점에 백그라운드 갱신

[output]
users_fields = ["id", "name", "real_name", "email"]
channels_fields = ["id", "name", "type", "members"]

[connection]
rate_limit_per_minute = 20
timeout_seconds = 30
```

**우선순위**: CLI 옵션 > 환경변수 > 설정 파일

---

## 명령어 참조

| 명령어 | 설명 |
|--------|------|
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
| `search <query>` | 메시지 검색 |
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
| `config init/show` | 설정 관리 |

### 공통 옵션
- `--json` — JSON 출력
- `--limit <N>` — 결과 제한
- `--thread <ts>` — 스레드 답장 (send)
- `--expand <fields>` — 추가 필드 (users/channels/messages)
  - users: `avatar`, `title`, `timezone`, `status`, `is_admin`, `is_bot`, `deleted`
  - channels: `topic`, `purpose`, `created`, `creator`, `is_archived`, `is_private`
  - messages: `date`, `user_name`

### messages 옵션
- `--oldest <date>` — 시작 시간 (Unix timestamp 또는 YYYY-MM-DD)
- `--latest <date>` — 종료 시간 (Unix timestamp 또는 YYYY-MM-DD)
- `--exclude-bots` — 봇 메시지 제외

---

## 문제 해결

### 캐시 초기화
```bash
rm -rf ~/.config/slack-cli/cache && slack-cli cache refresh
```

### 권한 오류
토큰 scope 확인 → Workspace 재설치

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
