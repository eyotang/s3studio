# S3 Studio

[简体中文](README.md) · [English](README.en.md) · [한국어](README.ko.md) · [日本語](README.ja.md) · [Français](README.fr.md) · [Deutsch](README.de.md) · [Русский](README.ru.md)

S3 Studio는 클라우드 파일을 하나의 깔끔한 작업 공간에서 관리하는 크로스 플랫폼 데스크톱 S3 클라이언트입니다. AWS S3, MinIO 및 기타 S3 호환 서비스에 연결하여 여러 계정과 버킷을 관리하고 파일을 업로드하거나 다운로드할 수 있습니다. 계정 수는 무제한이며 자격 증명은 시스템 키체인에 보관됩니다.

![S3 Studio](artifacts/workspace-preview.png)

## 주요 기능

- 사용자 지정 Endpoint, Region, 경로 스타일 및 임시 자격 증명을 지원하는 다중 계정 관리.
- 폴더 탐색, 경로 표시, 검색, 정렬, 목록·격자 보기, 다중 선택 및 객체 상세 정보.
- 업로드·다운로드, 폴더·버킷 생성, 버킷 내 복사, 일괄 삭제 및 서명된 다운로드 링크 공유.
- 전송 진행률과 속도, 일시 중지, 재개, 재시도 및 앱 재시작 후 이어받기.
- 모든 계정이 공유하는 동시 전송 수: 기본 8, 1–32로 조정 가능.
- 시스템·라이트·다크 테마와 중국어, 영어, 한국어, 일본어, 프랑스어, 독일어, 러시아어.
- macOS Keychain, Windows Credential Manager 또는 Linux Secret Service에 자격 증명 저장.

## 다운로드 및 설치

[GitHub Releases](https://github.com/eyotang/s3studio/releases/latest)에서 최신 버전을 다운로드하세요. 설치 전에 해당 릴리스의 `SHA256SUMS.txt`로 파일을 확인하세요.

| 플랫폼                      | 패키지           | 설치 방법                             |
| --------------------------- | ---------------- | ------------------------------------- |
| macOS Apple Silicon / Intel | Universal `.dmg` | S3 Studio를 응용 프로그램으로 드래그  |
| Windows x64                 | `.exe` / `.msi`  | 설치 프로그램 실행                    |
| Ubuntu 22.04+ amd64         | `.deb`           | 시스템 패키지 설치기로 열기           |
| Linux x86_64                | `.AppImage`      | 실행 권한을 부여한 뒤 실행            |
| Omarchy / Arch Linux x86_64 | `.pkg.tar.zst`   | `sudo pacman -U`와 패키지 경로로 설치 |

### macOS에서 앱을 열 수 없는 경우

현재 설치 파일에는 운영체제 코드 서명 및 Apple 공증이 적용되지 않았습니다. 앱을 응용 프로그램 폴더로 옮기고 공식 릴리스 출처와 SHA-256을 확인한 후 다음 명령을 실행하세요:

```bash
sudo xattr -d com.apple.quarantine "/Applications/S3 Studio.app"
```

Linux에서는 GNOME Keyring 등 Secret Service 키체인을 실행하고 잠금을 해제해야 하며 데스크톱 파일 선택 포털도 필요합니다. AppImage는 파일 속성에서 실행을 허용하세요. Omarchy 패키지는 AUR에 없으므로 새 패키지를 다운로드하여 `pacman -U`로 업데이트하세요. `sudo pacman -R s3-studio`로 제거해도 개인 설정은 유지됩니다.

## 시작하기

### 계정 연결

계정 추가를 선택하고 이름, Access Key ID, Secret Access Key, Region을 입력하세요. AWS는 보통 Endpoint를 입력하지 않아도 되며 버킷 리전을 자동 감지합니다. 임시 자격 증명에는 Session Token도 필요합니다. MinIO 예: Endpoint `http://localhost:9000`, Region `us-east-1`, 경로 스타일 활성화. 운영 환경에서는 HTTPS를 사용하세요.

각 작업에 필요한 S3 권한이 있어야 합니다. 폴더는 더블 클릭하여 탐색하고 파일은 더블 클릭하여 상세 정보를 확인합니다. 객체를 선택하여 다운로드, 복사, 공유 또는 삭제할 수 있습니다. 폴더 삭제는 확인 후 모든 내용을 재귀 삭제합니다. 업로드는 같은 이름의 객체를 덮어씁니다. 공유 링크는 1시간 유효하며 임시 자격 증명의 만료로 더 일찍 만료될 수 있습니다.

### 언어, 테마 및 전송

사이드바의 설정 또는 macOS의 S3 Studio → 설정…(⌘,)을 여세요. 언어와 테마는 즉시 적용되고 로컬에 저장됩니다. 시스템 설정 따르기는 지원되는 시스템 언어를 감지하고 그 외에는 영어를 사용합니다. 동시 전송 수를 1–32로 설정한 뒤 설정 저장을 누르세요. macOS 기본 제목 표시줄도 선택한 테마를 따릅니다.

전송 활동에서 일시 중지, 재개, 재시도 또는 취소할 수 있습니다. 일시 중지 시 완료된 파트를 유지합니다. 앱을 재시작해도 미완료 전송은 직접 재개하기 전까지 중지 상태입니다. 취소하면 체크포인트와 임시 내용을 정리합니다. 원본·원격 객체·대상 파일이 변경되면 안전하지 않은 재개나 덮어쓰기를 차단할 수 있습니다.

### 업데이트 및 제한

macOS 앱 메뉴나 버전 표시를 눌러 업데이트를 확인하세요. 정식 버전은 시작 20초 후와 이후 24시간마다 확인합니다. Windows NSIS/MSI 및 Linux AppImage는 검증된 앱 내 설치를 지원하며 먼저 전송을 완료하거나 일시 중지해야 합니다. macOS 및 deb는 릴리스 페이지에서 수동 설치합니다. 계정과 체크포인트는 유지됩니다.

업로드는 파일당 최대 5 TiB이며 서버 내 단일 객체 복사는 S3 CopyObject의 5 GB 제한을 따릅니다. 삭제는 버전 기록을 제거하지 않습니다. SSO/AssumeRole, ACL·정책 편집, 버전 관리, 계정 간 복사, 디렉터리 동기화, Glacier 복원, 폴더 다운로드 및 CloudFront 관리는 포함되지 않습니다. S3 호환 서비스의 동작은 서비스 구현에 따라 달라집니다.

## 개발 및 기여

GPUI, GPUI Kit, Rust 및 AWS SDK for Rust를 사용합니다. `src-gpui`는 기본 UI, `src-core`는 공유 백엔드이며 `src`/`src-tauri`에는 원래 React/Tauri 참조 구현이 있습니다. Node.js 22.12+와 안정 버전 Rust가 필요합니다. macOS는 Xcode Command Line Tools, Windows는 MSVC C++ Build Tools와 WebView2가 필요합니다. Linux 기본 라이브러리는 [개발 참조](README.md#开发与贡献)를 확인하세요.

```bash
npm ci
npm run dev:native
```

예시 데이터로 데스크톱 UI를 보려면 `npm run preview:native`를 실행하세요. `npm run dev`는 `http://127.0.0.1:1420`에서 브라우저 참조 화면을 실행하며 클라우드 연결이나 자격 증명 저장을 하지 않습니다.

변경 사항과 관련된 검사를 실행하세요:

```bash
npm test
npm run test:ui
npm run build
cargo test --locked --manifest-path src-core/Cargo.toml
cargo test --locked --manifest-path src-gpui/Cargo.toml
```

`npm run build:native`로 실행 파일을 빌드하고 `npm run tauri bundle -- --ci`로 패키징합니다. 각 플랫폼의 패키지는 해당 운영체제에서 빌드합니다. Linux 의존성, Universal 빌드, 저장 경로 및 릴리스 서명은 [개발 참조](README.md#开发与贡献)를 확인하세요. `src-core/locales`의 번역을 수정한 뒤 `node scripts/sync-i18n.mjs`를 실행하고 Rust 형식을 정리한 다음 테스트하세요.

Issue에는 앱 버전, 운영체제, 재현 단계와 오류를 포함하세요. 로그와 스크린샷에서 자격 증명과 개인정보를 제거하세요. Pull Request에는 문제, 변경 후 동작과 검증 결과를 설명하고 UI 변경 시 스크린샷을 첨부하세요. 테스트는 실제 클라우드 계정 대신 로컬 S3 시뮬레이터를 사용합니다. 릴리스 시 [AGENTS.md](AGENTS.md)를 따라 영어 설명을 먼저, 중국어를 다음에 작성하고 양쪽에 macOS xattr 안내를 포함하세요.

이 프로젝트는 독립적으로 구현되었으며 AWS 공식 제품이 아닙니다. [중국어 기술 참조](README.md), [릴리스 노트](docs/releases), [브랜드 리소스](assets/brand)를 참고하세요.
