# Slack CLI

[![CI](https://github.com/junyeong-ai/slack-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/slack-cli/actions)
[![Lint](https://github.com/junyeong-ai/slack-cli/workflows/Lint/badge.svg)](https://github.com/junyeong-ai/slack-cli/actions)
[![Rust](https://img.shields.io/badge/rust-1.91.1%2B%20(2024%20edition)-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-0.1.0-blue?style=flat-square)](https://github.com/junyeong-ai/slack-cli/releases)

> **🌐 한국어** | **[English](README.en.md)**

---

> **⚡ 빠르고 강력한 Slack 명령줄 도구**
>
> - 🚀 **밀리초 단위 검색** (SQLite FTS5 전문 검색)
> - 💾 **로컬 캐시** (사용자/채널 즉시 조회)
> - 🔍 **퍼지 매칭** (오타에도 정확한 검색)
> - 🛠️ **9개 명령어** (검색, 메시지, 설정 관리)

---

## ⚡ 빠른 시작 (1분)

```bash
# 1. 설치
git clone https://github.com/junyeong-ai/slack-cli
cd slack-cli
cargo build --release

# 2. 전역 설치 (선택사항)
./scripts/install.sh

# 3. 설정 초기화
slack-cli config init --bot-token xoxb-your-token

# 4. 캐시 새로고침
slack-cli cache refresh

# 5. 사용 시작! 🎉
slack-cli users "john"
slack-cli channels "general"
slack-cli send "#general" "Hello team!"
```

**Tip**: User token (`xoxp-`)을 사용하면 더 많은 기능을 사용할 수 있습니다.

---

## 🎯 주요 기능

### 강력한 검색
```bash
# 사용자 검색 (이름, 이메일, 표시명)
slack-cli users "john" --limit 5

# 채널 검색 (이름, 주제, 설명)
slack-cli channels "dev" --limit 10

# 메시지 검색 (워크스페이스 전체)
slack-cli search "deadline" --channel "#dev-team"
```

### 메시지 관리
```bash
# 채널에 메시지 전송
slack-cli send "#general" "Meeting in 10 minutes"

# DM 전송
slack-cli send "@john.doe" "Hello!"

# 스레드 답장
slack-cli send "#dev-team" "Done!" --thread 1234567890.123456

# 채널 메시지 조회
slack-cli messages "#general" --limit 20

# 스레드 전체 조회
slack-cli thread "#dev-team" 1234567890.123456
```

### 채널 관리
```bash
# 채널 멤버 목록
slack-cli members "#dev-team"

# JSON 출력
slack-cli channels "general" --json | jq
```

### 캐시 & 설정
```bash
# 캐시 상태 확인
slack-cli cache stats

# 캐시 새로고침
slack-cli cache refresh           # 전체
slack-cli cache refresh users     # 사용자만
slack-cli cache refresh channels  # 채널만

# 설정 관리
slack-cli config show            # 설정 표시 (토큰 마스킹)
slack-cli config path            # 설정 파일 경로
slack-cli config edit            # 에디터로 수정
```

**중요 사항**:
- 캐시가 오래됨 (>24h): 검색은 오래된 데이터 반환. `slack-cli cache refresh`로 갱신
- `search` 명령어: 캐시 미사용, API 직접 호출. User token + `search:read` scope 필요
- 채널 형식: `#channel-name`, `@username`, 또는 ID (`C123...`, `U456...`). ID에는 prefix 선택사항

---

## 📦 설치

### 방법 1: Prebuilt Binary (권장) ⭐

**자동 설치**:
```bash
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/install.sh | bash
```

**수동 설치**:
1. [Releases](https://github.com/junyeong-ai/slack-cli/releases)에서 바이너리 다운로드
2. 압축 해제: `tar -xzf slack-*.tar.gz`
3. PATH에 이동: `mv slack-cli ~/.local/bin/`

### 방법 2: Cargo

```bash
cargo install slack-cli
```

### 방법 3: 소스 빌드

```bash
git clone https://github.com/junyeong-ai/slack-cli
cd slack-cli
./scripts/install.sh
```

**Requirements**: Rust 1.91.1+

### 🤖 Claude Code Skill (선택사항)

`./scripts/install.sh` 실행 시 Claude Code 스킬 설치 여부를 선택할 수 있습니다:

- **User-level** (권장): 모든 프로젝트에서 사용 가능
- **Project-level**: Git을 통해 팀 자동 배포
- **Skip**: 나중에 수동 설치

스킬을 설치하면 Claude Code에서 자연어로 Slack 데이터 조회가 가능합니다.

---

## 🔑 Slack 토큰 생성

### User Token (권장) ⭐

1. [api.slack.com/apps](https://api.slack.com/apps) 접속
2. "Create New App" → "From scratch"
3. **User Token Scopes** 추가:
   ```
   channels:read channels:history groups:read groups:history
   im:read im:history mpim:read mpim:history
   users:read users:read.email chat:write search:read
   ```
4. "Install to Workspace" → 토큰 복사 (`xoxp-`로 시작)

### Bot Token (대안)

1. 위와 동일한 앱 생성
2. **Bot Token Scopes** 추가:
   ```
   channels:read channels:history groups:read groups:history
   im:read im:history mpim:read mpim:history
   users:read users:read.email chat:write
   ```
3. "Install to Workspace" → 토큰 복사 (`xoxb-`로 시작)

### 토큰 비교

| 기능 | User Token ⭐ | Bot Token |
|------|--------------|-----------|
| 채널 접근 | ✅ 자동 | ⚠️ 초대 필요 |
| 메시지 검색 | ✅ 가능 | ❌ 불가능 |
| 발신자 | 본인 | 봇 계정 |

---

## ⚙️ 설정

### 환경 변수

```bash
export SLACK_BOT_TOKEN="xoxb-..."      # 봇 토큰
export SLACK_USER_TOKEN="xoxp-..."    # 사용자 토큰 (권장)
```

### 설정 파일

**위치**:
- macOS: `~/.config/slack-cli/config.toml`
- Linux: `~/.config/slack-cli/config.toml`
- Windows: `%APPDATA%\slack-cli\config.toml`

**기본 설정** (`slack-cli config init`로 생성):
```toml
bot_token = "xoxb-..."
user_token = "xoxp-..."

[cache]
ttl_users_hours = 24
ttl_channels_hours = 24
data_path = "~/.config/slack-cli/cache"  # 모든 플랫폼 동일

[retry]
max_attempts = 3
initial_delay_ms = 1000
max_delay_ms = 60000

[connection]
timeout_seconds = 30
max_idle_per_host = 10
```

### 설정 우선순위

```
CLI 플래그 > 환경 변수 > 설정 파일 > 기본값
```

**예시**:
```bash
# 설정 파일의 토큰 오버라이드
slack-cli users "john" --token xoxp-temporary-token
```

---

## 🏗️ 핵심 구조

SQLite FTS5로 빠른 로컬 검색 (<10ms), 사용자/채널 24시간 캐시, API 호출 속도 제한.
상세한 아키텍처는 [CLAUDE.md](CLAUDE.md) 참고.

---

## 🔧 문제 해결

### 캐시가 갱신되지 않음

```bash
# 캐시 삭제 후 재생성
rm -rf ~/.config/slack-cli/cache

# 다시 실행
slack-cli cache refresh
```

### "Unauthorized" 오류

**확인 사항**:
- [ ] 토큰 형식 확인 (`xoxp-` 또는 `xoxb-`)
- [ ] 필수 scope 추가 확인
- [ ] Workspace 재설치 확인

**토큰 테스트**: Slack API `auth.test` 엔드포인트로 검증

### 메시지 검색 안 됨

**원인**: User token 없거나 `search:read` scope 없음

**해결**:
1. `SLACK_USER_TOKEN` 설정 (`xoxp-`)
2. `search:read` scope 추가
3. Workspace 재설치

### 디버그 로깅

`RUST_LOG` 환경변수로 디버그 로깅 활성화 (예: `RUST_LOG=debug slack-cli users "john"`)

### 캐시 데이터 확인

```bash
# SQLite로 직접 캐시 검사
sqlite3 ~/.config/slack-cli/cache/slack.db
```

---

## 📚 명령어 참조

| 명령어 | 설명 | 예제 |
|--------|------|------|
| `users <query>` | 사용자 검색 (이름, 이메일, 표시명) | `slack-cli users "john" --limit 5` |
| `channels <query>` | 채널 검색 (공개/비공개/DM/그룹 DM) | `slack-cli channels "dev" --limit 10` |
| `send <channel> <text>` | 메시지 전송 | `slack-cli send "#general" "Hello!"` |
| `messages <channel>` | 채널 메시지 조회 | `slack-cli messages "#general" --limit 20` |
| `thread <channel> <ts>` | 스레드 전체 조회 | `slack-cli thread "#dev" 1234567890.123456` |
| `members <channel>` | 채널 멤버 목록 | `slack-cli members "#dev-team"` |
| `search <query>` | 메시지 검색 (워크스페이스 전체) | `slack-cli search "deadline" --channel "#dev"` |
| `cache stats` | 캐시 통계 (사용자/채널 개수) | `slack-cli cache stats` |
| `cache refresh` | 캐시 새로고침 (전체/사용자/채널) | `slack-cli cache refresh users` |
| `config init` | 설정 초기화 | `slack-cli config init --bot-token xoxb-...` |
| `config show` | 설정 표시 (토큰 마스킹) | `slack-cli config show` |

### 공통 옵션

| 옵션 | 설명 | 적용 범위 |
|------|------|-----------|
| `--json` | JSON 형식으로 출력 | 모든 명령어 |
| `--token <TOKEN>` | 임시 토큰 오버라이드 | 모든 명령어 |
| `--limit <N>` | 결과 개수 제한 | users, channels, messages, thread, search |
| `--thread <TS>` | 스레드 타임스탬프 (답장) | send |
| `--channel <CH>` | 특정 채널로 제한 | search |

**참고**:
- `search` 명령어는 User token (`xoxp-`) + `search:read` scope 필요
- `cache refresh`는 `users` 또는 `channels` 인자로 부분 갱신 가능 (예: `slack-cli cache refresh users`)
- 타임스탬프 형식: `1234567890.123456` (Slack 메시지 ts 값)

---

## 🚀 개발자 가이드

**아키텍처, 디버깅, 기여 방법**: [CLAUDE.md](CLAUDE.md) 참고

---

## 💬 지원

- **GitHub Issues**: [문제 신고](https://github.com/junyeong-ai/slack-cli/issues)
- **개발자 문서**: [CLAUDE.md](CLAUDE.md)

---

<div align="center">

**🌐 한국어** | **[English](README.en.md)**

**Version 0.1.0** • Rust 2024 Edition

Made with ❤️ for productivity

</div>
