pipeline {
    agent { label 'engine-linux' }
    options {
        disableConcurrentBuilds()
        timestamps()
        timeout(time: 60, unit: 'MINUTES')
        buildDiscarder(logRotator(numToKeepStr: '20', artifactNumToKeepStr: '5'))
    }
    triggers { pollSCM('H/2 * * * *') }
    environment {
        CI = 'true'
        CARGO_TERM_COLOR = 'never'
    }
    stages {
        stage('Dependencies') {
            steps {
                sh 'cargo fetch --locked'
                dir('apps/admin-dashboard') { sh 'npm ci --no-audit --no-fund' }
                dir('apps/editor') { sh 'npm ci --no-audit --no-fund' }
                dir('docs-site') { sh 'npm ci --no-audit --no-fund' }
            }
        }
        stage('Tests') {
            steps {
                catchError(buildResult: 'FAILURE', stageResult: 'FAILURE') {
                    sh 'python3 test_all.py'
                }
            }
        }
        stage('Frontend builds') {
            parallel {
                stage('Admin') {
                    steps { dir('apps/admin-dashboard') { sh 'npm run build' } }
                }
                stage('Editor apps') {
                    stages {
                        stage('Editor design registry tests') {
                            steps { dir('apps/editor') { sh 'npm run test:design-registry' } }
                        }
                        stage('Editor development and staging builds') {
                            steps {
                                dir('apps/editor') {
                                    sh 'npm run build:dev'
                                    sh 'npm run build:staging'
                                }
                            }
                        }
                        stage('Editor design guide builds') {
                            steps {
                                dir('apps/editor') {
                                    sh 'npm run build:design'
                                    sh 'npm run build:design:staging'
                                }
                            }
                        }
                        stage('Editor production build and isolation check') {
                            steps { dir('apps/editor') { sh 'npm run check:production' } }
                        }
                    }
                }
            }
        }
        stage('Rust app builds') {
            steps {
                sh 'cargo build --workspace --all-targets --features engine-admin/desktop --offline --locked'
            }
        }
        stage('Admin desktop tests') {
            steps {
                sh 'cargo test -p engine-admin --features desktop --all-targets --offline --locked'
            }
        }
        stage('Documentation site checks') {
            steps { dir('docs-site') { sh 'npm test' } }
        }
    }
    post {
        success {
            archiveArtifacts artifacts: 'apps/admin-dashboard/dist/**,apps/editor/dist/**,docs-site/dist/**', fingerprint: true
        }
    }
}
