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
                stage('Editor') {
                    steps { dir('apps/editor') { sh 'npm run build' } }
                }
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
