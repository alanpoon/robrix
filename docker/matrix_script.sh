docker exec -it docker-synapse-1 register_new_matrix_user http://localhost:8008 -c /data/homeserver.yaml

cargo run --bin matrix-login -- -s http://localhost:8008 -u testuser -p testpassword -t 30

cargo run --bin matrix-login -- -s http://localhost:8008 -u testuser2 -p testpassword -t 30