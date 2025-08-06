class WebTerminal {
    constructor() {
        this.terminal = null;
        this.websocket = null;
        this.fitAddon = null;
        this.currentLine = '';
        
        this.init();
    }

    async init() {
        await this.fetchShellInfo();
        this.setupTerminal();
        this.connectWebSocket();
        this.setupEventListeners();
    }

    setupTerminal() {
        this.terminal = new Terminal({
            cursorBlink: true,
            theme: {
                background: '#1e1e1e',
                foreground: '#ffffff',
                cursor: '#ffffff',
                selection: 'rgba(255, 255, 255, 0.3)',
                black: '#000000',
                red: '#cd3131',
                green: '#0dbc79',
                yellow: '#e5e510',
                blue: '#2472c8',
                magenta: '#bc3fbc',
                cyan: '#11a8cd',
                white: '#e5e5e5',
                brightBlack: '#666666',
                brightRed: '#f14c4c',
                brightGreen: '#23d18b',
                brightYellow: '#f5f543',
                brightBlue: '#3b8eea',
                brightMagenta: '#d670d6',
                brightCyan: '#29b8db',
                brightWhite: '#e5e5e5'
            },
            fontSize: 14,
            fontFamily: '"Fira Code", "Cascadia Code", "Menlo", "Monaco", monospace',
            cols: 80,
            rows: 24
        });

        this.fitAddon = new FitAddon.FitAddon();
        this.terminal.loadAddon(this.fitAddon);
        
        const terminalElement = document.getElementById('terminal');
        this.terminal.open(terminalElement);
        
        setTimeout(() => {
            this.fitAddon.fit();
        }, 100);

        this.terminal.onData(data => {
            if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
                this.websocket.send(data);
            }
        });

        this.terminal.writeln('Welcome to Web Terminal');
        this.terminal.writeln('Connecting to server...');
    }

    connectWebSocket() {
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/ws`;
        
        this.websocket = new WebSocket(wsUrl);
        
        this.websocket.onopen = () => {
            this.terminal.clear();
            this.terminal.writeln('Connected to terminal server');
            console.log('WebSocket connected');
        };
        
        this.websocket.onmessage = (event) => {
            // 确保正确处理换行符
            let data = event.data.replace(/\n/g, '\r\n');
            this.terminal.write(data);
        };
        
        this.websocket.onclose = () => {
            this.terminal.writeln('\r\n\nConnection closed. Attempting to reconnect...');
            console.log('WebSocket closed');
            setTimeout(() => {
                this.connectWebSocket();
            }, 3000);
        };
        
        this.websocket.onerror = (error) => {
            this.terminal.writeln('\r\n\nConnection error occurred');
            console.error('WebSocket error:', error);
        };
    }

    async fetchShellInfo() {
        try {
            const response = await fetch('/api/shell-info');
            const shellInfo = await response.json();
            this.shellInfo = shellInfo;
            
            // 更新页面标题
            const titleElement = document.querySelector('.header-title');
            if (titleElement) {
                titleElement.textContent = `Web Terminal - ${shellInfo.shell}`;
            }
            
            console.log('Shell info loaded:', shellInfo);
        } catch (error) {
            console.error('Failed to fetch shell info:', error);
            this.shellInfo = { shell: 'bash', args: ['-i'], full_command: 'bash -i' };
        }
    }

    setupEventListeners() {
        window.addEventListener('resize', () => {
            if (this.fitAddon) {
                setTimeout(() => {
                    this.fitAddon.fit();
                }, 100);
            }
        });

        window.addEventListener('beforeunload', () => {
            if (this.websocket) {
                this.websocket.close();
            }
        });

        document.querySelector('.control-button.close').addEventListener('click', () => {
            if (confirm('Are you sure you want to close the terminal?')) {
                window.close();
            }
        });

        document.querySelector('.control-button.minimize').addEventListener('click', () => {
            document.body.style.transform = 'scale(0.8)';
            document.body.style.transformOrigin = 'top left';
        });

        document.querySelector('.control-button.maximize').addEventListener('click', () => {
            document.body.style.transform = 'scale(1)';
            document.body.style.transformOrigin = 'top left';
        });

        document.addEventListener('keydown', (e) => {
            if (e.ctrlKey && e.key === 'c') {
                e.preventDefault();
                this.sendKeyboardInterrupt();
            }
        });
    }

    sendKeyboardInterrupt() {
        if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
            this.websocket.send('\u0003'); // Ctrl+C
        }
    }

    focus() {
        if (this.terminal) {
            this.terminal.focus();
        }
    }
}

document.addEventListener('DOMContentLoaded', () => {
    const webTerminal = new WebTerminal();
    
    setTimeout(() => {
        webTerminal.focus();
    }, 500);
});