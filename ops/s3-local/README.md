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
