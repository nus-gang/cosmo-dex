# S2 문서 QA 운송

제품 `84ea15219512b84cdf0f57c59913ae99ed1f3fee`, 문서 `aad819f20add7595e449e21b83221063466565b5`를 별도 checkout한다. 이 디렉터리와 `.github/workflows/s2-docqa.yml`만 SRE 변경이다. 기본 `environment.mjs`는 환경 인수용이며 거래 안내의 QA PASS를 주장하지 않는다.

## QA 실행·소유권

새 표준 hosted runner의 원래 포트에서 유한 시험을 실행한다. 로컬 공유 runtime은 사용하지 않는다. `push`의 `sre/nus-50-docqa-transport` 또는 `qa/docqa/**` branch, 해당 파일을 바꾸는 PR에서 실행한다. main 병합이나 default-branch dispatch는 필요 없다. job은 최대25분, driver는600초, runtime 정상 종료 wait는40초다. Actions 취소는 정상 stop을 대체하지 않는다. 취소/timeout/정리 증거 누락은 INCOMPLETE/FAIL이며 새 실행이 필요하다.

QA는 독립 driver를 저장소의 `qa/docqa/**` branch에 commit한다. `input.json`의 `driver_ref`를 해당 **40자리 commit**으로, `driver_path`를 해당 모듈 경로로 지정한 별도 transport commit을 push한다. `SELF`는 운송 commit과 driver commit이 같은 초기 환경 시험만을 위한 명시적 선택이다. 각 실행 manifest는 product/docs/transport/driver SHA와 driver 파일 SHA256을 분리 기록한다. 새 driver 적용에는 새 push가 필요하며 기존 job rerun은 원래 입력을 유지한다.

`export default async function({page,control,output})` 모듈이 UI 조작·기대값·문서 판정을 소유한다. `page`는 새 context의 새 Chromium Page다. 테스트용 UI, API 응답 interception, 사용자 키 주입 없이 원래 UI를 조작한다. `control`은 순서대로 await한다.

| 호출 | 의미 |
|---|---|
| `control('temporary_start')` | 10개 포트가 비었는지 확인하고 임시 웹 기동 |
| `control('init',{publicKeys})` | UI에서 생성한 두 공개키로 새 home 한 번 init |
| `control('temporary_stop')` | 소유 임시 웹 종료, 5173 해제 |
| `control('start')` | 같은 home 통합 runtime 기동, 실제 4노드/API/web health 반환 |
| `control('health')` | 네 노드 높이/catching_up, API OPEN, web200 확인 |
| `control('stop')` | 소유 runtime TERM, 40초 wait, exit0/descendant/10 listeners/3 locks/socket 확인 |

`init` 반환값은 원래 runtime.json의 genesis/build pin이다. 키를 만든 Page에 hash를 넣고 로그인한다. 페이지 reload/goto는 최초 탐색 이후 금지한다. QA는 예치·부분 체결·취소·IOC·UNSETTLED_HOLD를 독립 검증한 뒤 stop/start, 같은 Page 재로그인과 원장/주문/fill 보존을 검증한다. 마지막에도 명시적으로 stop한다. 두 번 이상의 정상 통합 stop이 없는 driver는 운송 인수를 통과하지 못한다. 서버 기록은 같은 home에 보존되며 재init·삭제하지 않는다.

## 실제 entrypoint와 증거

runner 저장소 root에서:

```sh
python3 transport/tests/docqa/transport.py --product product --docs docs --driver-root qa --home "$RUNNER_TEMP/docqa/home" --output public
```

하위 argv는 `node transport/tests/docqa/browser.mjs <absolute-product> <absolute-driver> <absolute-public>`이며 cwd는 product다. `GITHUB_ACTIONS=true`인 격리 CI에서만 실행한다. workflow 표준 GITHUB_RUN_ID/ATTEMPT/ACTOR/TRIGGERING_ACTOR와 ImageOS/ImageVersion/RUNNER_OS/ARCH, tool PATH/HOME을 읽는다. 운영 secret이나 추가 키는 필요 없다. 공유 호스트에서 이 보호조건을 우회하지 않는다.

manifest에 실제 argv/cwd/refs/파일 hash/빌드 hash/버전/health/종료를 남긴다. browser.json은 환경 또는 독립 driver 결과, http.json은 URL origin/path·상태·CORS 허용 origin만 포함한다. Authorization·응답 body·브라우저 storage·trace는 수집하지 않는다. 공개키·두 화면·명시한 JSON 파일만 artifact로 업로드한다. node home·validator key·journal 원문·private 로그는 runner scratch에 남고 job 종료와 함께 폐기된다. QA가 추가 공개 증거를 만들면 workflow allowlist를 명시적으로 갱신한다.

실제 run/job/artifact ID, action resolved SHA 및 다운로드 digest는 Paperclip 인계 문서에서 제공한다. QA 자신의 연결에서:

```sh
gh run rerun --repo nus-gang/cosmo-dex --job <new-docqa-job-id>
gh run view <run-id> --repo nus-gang/cosmo-dex --json jobs,headSha,attempt,url
gh run download <run-id> --repo nus-gang/cosmo-dex --name s2-docqa-<run-id>-<attempt> --dir <new-evidence-dir>
```

기본 모듈 결과는 **환경만 PASS / 문서 QA NOT_RUN**이다. QA가 독립 기대값·driver를 검토하고 새 실행을 수행한 결과로 문서 판정을 기록한다. 환경 인계는 상위 QA 완료를 기다리지 않는다.
