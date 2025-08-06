class WebTerminal {
    constructor() {
        this.terminal = null;
        this.websocket = null;
        this.fitAddon = null;
        this.currentLine = '';
        this.connected = false;
        
        this.init();
    }

    async init() {
        await this.fetchShellInfo();
        this.setupTerminal();
        this.connectWebSocket();
        this.setupEventListeners();
        this.setupThemeController();
        this.updateConnectionStatus();
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
            this.connected = true;
            this.updateConnectionStatus();
            console.log('WebSocket connected');
        };
        
        this.websocket.onmessage = (event) => {
            // 确保正确处理换行符
            let data = event.data.replace(/\n/g, '\r\n');
            this.terminal.write(data);
        };
        
        this.websocket.onclose = () => {
            this.terminal.writeln('\r\n\nConnection closed.');
            this.connected = false;
            this.updateConnectionStatus();
            console.log('WebSocket closed');
            this.showReconnectDialog();
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
            
            // 更新页面标题和 shell 信息
            const titleElement = document.querySelector('.header-title');
            if (titleElement) {
                titleElement.textContent = `Web Terminal - ${shellInfo.shell}`;
            }
            
            const shellInfoElement = document.querySelector('.shell-info');
            if (shellInfoElement) {
                shellInfoElement.textContent = `Shell: ${shellInfo.shell} ${shellInfo.args.join(' ')}`;
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
            const container = document.querySelector('.container');
            container.style.transform = 'scale(0.9)';
            container.style.transition = 'transform 0.2s ease';
            setTimeout(() => {
                container.style.transform = 'scale(1)';
            }, 200);
        });

        document.querySelector('.control-button.maximize').addEventListener('click', () => {
            document.documentElement.requestFullscreen().catch(err => {
                console.log('Fullscreen not supported:', err);
            });
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
    
    updateConnectionStatus() {
        const badge = document.querySelector('.badge');
        
        if (this.connected) {
            badge?.classList.remove('badge-error');
            badge?.classList.add('badge-success');
            if (badge) badge.innerHTML = '<div class="w-2 h-2 rounded-full bg-success animate-pulse"></div>Connected';
        } else {
            badge?.classList.remove('badge-success');
            badge?.classList.add('badge-error');
            if (badge) badge.innerHTML = '<div class="w-2 h-2 rounded-full bg-error"></div>Disconnected';
        }
    }
    
    setupThemeController() {
        const themeControllers = document.querySelectorAll('.theme-controller');
        
        themeControllers.forEach(controller => {
            controller.addEventListener('click', (e) => {
                e.preventDefault();
                const theme = controller.getAttribute('data-theme');
                document.documentElement.setAttribute('data-theme', theme);
                
                // Update terminal theme based on DaisyUI theme
                this.updateTerminalTheme(theme);
                
                // Store theme preference
                localStorage.setItem('terminal-theme', theme);
            });
        });
        
        // Load saved theme
        const savedTheme = localStorage.getItem('terminal-theme') || 'dark';
        document.documentElement.setAttribute('data-theme', savedTheme);
        this.updateTerminalTheme(savedTheme);
    }
    
    updateTerminalTheme(theme) {
        if (!this.terminal) return;
        
        const themes = {
            'dark': {
                background: '#1f2937',
                foreground: '#f9fafb',
                cursor: '#3b82f6'
            },
            'light': {
                background: '#ffffff',
                foreground: '#111827',
                cursor: '#3b82f6'
            },
            'cyberpunk': {
                background: '#0a0a0a',
                foreground: '#00ff00',
                cursor: '#ff00ff'
            },
            'synthwave': {
                background: '#1a1a2e',
                foreground: '#ff6b9d',
                cursor: '#00d2ff'
            }
        };
        
        const selectedTheme = themes[theme] || themes.dark;
        
        this.terminal.options.theme = {
            ...this.terminal.options.theme,
            background: selectedTheme.background,
            foreground: selectedTheme.foreground,
            cursor: selectedTheme.cursor
        };
    }
    
    showReconnectDialog() {
        const modal = document.getElementById('reconnect_modal');
        const reconnectBtn = document.getElementById('reconnect-btn');
        
        // Remove existing event listeners to prevent duplicates
        const newReconnectBtn = reconnectBtn.cloneNode(true);
        reconnectBtn.parentNode.replaceChild(newReconnectBtn, reconnectBtn);
        
        // Add new event listener
        newReconnectBtn.addEventListener('click', () => {
            modal.close();
            this.terminal.writeln('Attempting to reconnect...');
            this.connectWebSocket();
        });
        
        modal.showModal();
    }
}

document.addEventListener('DOMContentLoaded', () => {
    const webTerminal = new WebTerminal();
    
    setTimeout(() => {
        webTerminal.focus();
    }, 500);
});