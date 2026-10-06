# L-R 사전 빌드 입력 도구

이 디렉터리는 [NUS-73](/NUS/issues/NUS-73)의 진행 중 산출물이다. **실행 launcher·최종 runtime manifest·pin은 아직 완성되지 않았다.** 서비스 시작 명령이 아니며 DEV01~14는 NOT_RUN이다.

`manifest.py audit`는 승인 L-E `6077a8397366a86d49b8e17eb12f988b8c21467f`와 A/B/C/L-D 포함 관계를 검증한다. 원 A Git snapshot에서 rc3 204파일, rc3 manifest, 개발 후보 8파일을 읽고 승인 SHA를 대조한다. 현재 구현 lock으로 rc3를 다시 봉인하지 않는다. 실제 구현 lock은 별도로 기록한다. clean checkout이 필요하다.

```sh
python3 ops/s3-local/manifest.py audit --source . --out /existing-parent/new-audit
python3 -B -m unittest discover -s ops/s3-local -p 'test_*.py' -v
```

`seal`은 `--build-spec spec.json --artifacts /build-root`를 추가로 받는다. spec의 키는 정확히 chain/exchange/settlement/wallet/sre다. 각 값에는 `build_argv`(문자열 배열), `toolchain`(문자열), `artifacts`(build-root 상대 파일 배열), `approval_sources`(출처 배열), `settings`(문자열 map)가 필요하다. 원본 lock/실제 lock·정확한 build argv·toolchain·파일 SHA가 5 descriptor의 문자열 설정에 결합된다. 실행 파일, 웹 파일과 launcher를 빠짐없이 열거하는 책임은 빌드 인계와 CTO→Security 심사에 있다. sealer는 경로가 있다고 실행 가능한 서비스인지를 보증하지 않는다.

파일 집합은 상속 213개와 descriptor 5개만이다. 실제 binary는 descriptor 안의 SHA로 결합하며 집계에 파일 자체를 추가하지 않는다. 집계는 경로 정렬 후 `sha256 + two spaces + path + LF`의 SHA256이다. manifest 자신·genesis·key·실행 결과를 집계에 넣지 않는다. 출력 디렉터리 재사용과 symlink/hardlink artifact를 거절한다.

codec가 요구하는 `scope=REVIEWED_RUNTIME`은 형식 필드이며 이 도구의 승인 선언이 아니다. `audit.json`의 `runtime_approved=false`, `candidate_runtime_manifest_sha256`은 검토 전 후보 기록이다. 이 값을 스스로 `approved_runtime_sha256`에 복사해 서비스를 시작하지 않는다. CEO/CTO의 독립 승인 출처와 동일 후보 CTO→Security 완료가 있어야 pin으로 인수한다. 한 바이트라도 바뀌면 새 manifest/심사가 필요하다.

검증 시험의 `NOT_A_RUNTIME_BINARY`와 `TEST_ONLY_NOT_APPROVED`는 합성 바이트 변조 시험이며 runtime 산출물로 인계하지 않는다. 새 서비스·home·genesis·키 생성, 표준 G00/ACK 변경, 새 설치나 main 병합을 수행하지 않는다.

## 실제 파일 바이트 preflight (기동 전 부분 구현)

```sh
python3 -B ops/s3-local/preflight.py \
  --bundle /absolute/candidate-bundle --artifacts /absolute/build-root \
  --runtime-pin <independently-approved-manifest-sha256> \
  --local-demo-profile s3-dev-local/1 --acknowledge-unproven-space
```

이 명령은 파일 읽기만 한다. 두 opt-in·manifest 원문 SHA·고정 A/rc3 manifest·상속 213파일과 정확한 5 descriptor·집계·descriptor 내부 실제 artifact SHA를 확인한다. canonical root·상대 경로·각 경로의 no-follow fd 탐색·single regular file을 요구하며 symlink/hardlink/FIFO·파일 변경·크기 초과를 거절한다. 산출물당 512MiB 상한, 해시 메모리는 1MiB다. 네트워크·port bind·home/key 생성·서비스 실행·승인 발급은 없다.

성공 출력은 `byte_match=true`, **`approval_verified=false`**다. 입력 pin의 독립 승인 여부와 CTO→Security 판정은 control plane에서 별도로 확인해야 한다. B/C의 guard/genesis/profile 검증, 실행 직전 재대조, 실제 runtime 등록을 대체하지 않는다. 검사 후 파일 변경 가능성이 있으므로 이 결과만으로 나중 실행을 허가하지 않는다. exact executable 목록의 완전성은 최종 후보 심사 대상이다.

`python3 -B -m unittest discover -s ops/s3-local -p test_preflight.py -v`에서 **11 PASS / 0 FAIL**. 승인 상속 파일을 Git A에서 읽고 합성 descriptor·`NOT_A_RUNTIME_BINARY`를 사용한 순수 파일 시험이다. 이 fixture pin은 승인 runtime pin이 아니다. 원문/산출물 변조, reseal한 상속 파일 교체, component 경로, 두 opt-in, 중복 JSON, alias/링크/FIFO/크기 상한 거절을 검증한다. 서비스0·DEV NOT_RUN이다.

worker/proof/signer·웹 ChainPort·프로세스 launcher·fee0/25 초기화·정리/fault driver·최종 manifest 및 독립 승인은 아직 남아 있다.
