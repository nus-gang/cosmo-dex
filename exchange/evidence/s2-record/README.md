# S2-C signed journal record 체크포인트

실제 ML-DSA fixture의 매도→부분 체결 매수→잔고 부족 거절→취소 4개 명령을 canonical JournalRecord로 투영했다. 원문/서명/hash/관측 context/전후 state hash/결과/held outbox를 같은 frame에 보존한다. 전체 frame hash와 seq가 일치하는 commit에서만 영수증을 구성한다. 메모리 후보와 내구성 증명은 별개이며 SignedRecord는 서비스가 아니다.

새 디렉터리의 실제 fsync journal에 4개를 기록하고 재열기 후 원 서명 입력을 다시 실행하여 record bytes, result/state hash, 원 영수증이 동일함을 확인했다. 재시도에는 새 record를 생성하지 않는다. 매칭의 4개 자산 행 변화와 양 주문 목록, 거절의 무변경, 취소 후에도 최초 LOCAL_ACCEPTED 영수증 보존을 검사했다. 잘못된 commit 및 중복 후보의 record 생성은 거절했다.

검증: s2_sequencer 20개 PASS(신규 통합 1개, 0.51초), 계약 fixture schema oracle 16개 PASS, all-targets clippy PASS. 일반 jsonschema runtime validation이 아니다. 처음 잘못 참조한 fixture 이름과 clippy 문자열 비교 경고를 수정하고 최종 재시험했다. macOS SDK 조회 sandbox 경고는 있었으나 빌드와 시험은 성공했다.

재현: RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo cargo test --locked --offline --manifest-path exchange/Cargo.toml --test s2_sequencer. S2_RECORD_SCHEMA_OUTPUT 환경변수를 지정하면 4개 record/result/receipt fixture를 출력한다. python3 exchange/tools/check_s2_projection.py <출력파일>. cargo clippy --locked --offline --manifest-path exchange/Cargo.toml --all-targets -- -D warnings.

제한: 합성 chain snapshot/시험 키다. 생산 서비스의 전역 gate·원 receipt index·디스크 semantic replay 자동화·withdraw local ID·내부 snapshot/correction record·최대 정정 크기 계산·crash/API 통합은 미완료다. 시험은 correction ceiling을 MAX_PAYLOAD로 전달했으며 실제 최대 정정 산출을 증명하지 않는다. 서비스 LOCAL_ACCEPTED, 운영용 ACK, 제품 PASS나 전문 심사를 주장하지 않는다. 처리량/CPU/RSS와 원격 CI는 미측정/미확인. 추가 유료 비용 없음.

GitHub 연결 needs_user_action으로 이번 patch는 아직 PR #27에 푸시하지 않았다. 연결 후 같은 브랜치에서 이어간다.
