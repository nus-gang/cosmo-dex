# 운영자·관리자 private 파일 게시

`localkeys.Authorities.Publish(absolutePath)`는 현재 uid 소유의 canonical
0700 부모 아래 새 0700 디렉터리만 만든다. operator-0/operator.seed,
operator-1/operator.seed, administrator/admin.seed는 각각 32바이트·0600이다.
기존 Rust LocalSigner의 파일명/길이/권한 형식을 따른다. 새로 게시한
운영자 seed를 실제 Rust LocalSigner로 읽고 서명한 뒤 Go SDK로 검증했다.

각 파일/role 디렉터리·root·부모 fsync를 확인하며, 오류 후 부분 파일은
보존한다. 같은 경로 자동 재시도·덮어쓰기·삭제는 없다. Publish는 성공/실패
모두 Authorities.Destroy를 호출한다. 외부로 이미 복사한 seed와 Go heap의
완전 소거는 보장하지 않는다. 반환 오류에는 원 seed/OS 경로를 넣지 않는다.

새 운영자·관리자 키 생성부터 파일 게시까지의 내부 API다. Context/guard,
B/C 의미 검증, 명령 진입점, runtime 승인은 별도 연결이 필요하다. 실제
서비스에 공개 fixture 키를 쓰지 않는다. 사용자 탭 키를 서버에 저장하지 않는다.

검증: Go1.26.5 offline/readonly, localkeys 신규3+회귀10=13 PASS.
fee0/25 키 원문·권한·소비, 기존/링크/잘못된 부모 거절, fsync 실패 뒤
부분 파일 보존·후속 쓰기0·재시도 거절. 임시 시험 키는 t.TempDir에서 제거된다.
실제 서비스·listener·RPC0, runtime pin 미발급, DEV NOT_RUN.

교차 검증: `NUS_AUTHORITY_TEST_BRIDGE`에 `authority_bridge_test.rs`의
컴파일 결과 절대 경로를 지정하고 위 localkeys 시험을 실행한다. 미지정 시
교차 시험은 SKIP이므로 PASS로 집계하지 않는다. 이번 실제 실행은 신규1+
기존13=14 PASS/0 FAIL/SKIP0. fee0/25 × operator0/1의 새 키·Rust 서명·
Go SDK 검증, 메시지 변조·다른 공개키·0644 seed 거절 및 권한 복원 후 성공을
확인했다. bridge는 시험 전용이며 runtime manifest/실행 명령에 포함하지 않는다.
관리자 서명과 Context/guard·B/C 전체 초기화는 이 시험의 대상이 아니다.
private 키는 시험 임시 디렉터리에서 제거되며 첨부/로그에 저장하지 않는다.
